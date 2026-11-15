// SPDX-License-Identifier: AGPL-3.0-only
// Capita Money – Android shell: reading the chip of a passport or identity
// card (ICAO 9303) for the document check (docs/adr/0021-verification-ways.md).
//
// The plugin reads EF.SOD, DG1 (machine-readable zone), DG2 (photo), and
// DG14/DG15 where present, and lets the chip prove itself (ADR 0024): chip
// authentication where DG14 offers it (the files after it are read under
// the chip's new session keys, so a copy of the files cannot answer), and
// active authentication of the service's chip nonce where DG15 exists (the
// answer goes to the service, which checks it). It keeps nothing; the bytes
// go to the identity service, which checks them (crates/capita-mrtd).
// Access: PACE where the chip offers it, else BAC – with the key printed on
// the document (number, date of birth, expiry), or the 6-digit card access
// number (CAN).

package com.capitamoney.app;

import android.nfc.NfcAdapter;
import android.nfc.Tag;
import android.nfc.tech.IsoDep;
import android.util.Base64;

import com.getcapacitor.JSObject;
import com.getcapacitor.Plugin;
import com.getcapacitor.PluginCall;
import com.getcapacitor.PluginMethod;
import com.getcapacitor.annotation.CapacitorPlugin;

import net.sf.scuba.smartcards.CardService;

import org.bouncycastle.jce.provider.BouncyCastleProvider;
import org.jmrtd.AccessKeySpec;
import org.jmrtd.BACKey;
import org.jmrtd.PACEKeySpec;
import org.jmrtd.PassportService;
import org.jmrtd.lds.CardAccessFile;
import org.jmrtd.lds.ChipAuthenticationInfo;
import org.jmrtd.lds.ChipAuthenticationPublicKeyInfo;
import org.jmrtd.lds.PACEInfo;
import org.jmrtd.lds.SecurityInfo;
import org.jmrtd.lds.icao.DG14File;
import org.jmrtd.lds.icao.DG15File;
import org.jmrtd.protocol.AAResult;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.math.BigInteger;
import java.security.PublicKey;
import java.security.Security;

@CapacitorPlugin(name = "CapitaChip")
public class CapitaChipPlugin extends Plugin {

    static {
        // JMRTD needs the full BouncyCastle provider; Android ships a reduced one.
        Security.removeProvider("BC");
        Security.insertProviderAt(new BouncyCastleProvider(), 1);
    }

    private PluginCall pending;
    private AccessKeySpec key;
    private byte[] chipNonce;

    /** Whether the phone can read chips at all, and whether NFC is switched on. */
    @PluginMethod
    public void status(PluginCall call) {
        NfcAdapter adapter = NfcAdapter.getDefaultAdapter(getContext());
        JSObject out = new JSObject();
        out.put("nfc", adapter != null);
        out.put("enabled", adapter != null && adapter.isEnabled());
        call.resolve(out);
    }

    /**
     * Waits for a document at the back of the phone and reads it.
     * Arguments: documentNumber, birthDate (YYMMDD), expiryDate (YYMMDD) – or can.
     * Answers base64 of sod, dg1, dg2 and, if present, dg14, dg15; and "pace".
     */
    @PluginMethod
    public void read(PluginCall call) {
        NfcAdapter adapter = NfcAdapter.getDefaultAdapter(getContext());
        if (adapter == null) { call.reject("This phone cannot read document chips.", "no-nfc"); return; }
        if (!adapter.isEnabled()) { call.reject("NFC is switched off.", "nfc-off"); return; }
        String can = call.getString("can");
        if (can != null && !can.isEmpty()) {
            key = PACEKeySpec.createCANKey(can);
        } else {
            String number = call.getString("documentNumber");
            String birth = call.getString("birthDate");
            String expiry = call.getString("expiryDate");
            if (number == null || birth == null || expiry == null) {
                call.reject("Document number, date of birth and expiry are needed.", "no-key");
                return;
            }
            key = new BACKey(number, birth, expiry);
        }
        String nonce = call.getString("chipNonce");
        chipNonce = nonce == null ? null : Base64.decode(nonce, Base64.DEFAULT);
        pending = call;
        call.setKeepAlive(true);
        int flags = NfcAdapter.FLAG_READER_NFC_A | NfcAdapter.FLAG_READER_NFC_B | NfcAdapter.FLAG_READER_SKIP_NDEF_CHECK;
        adapter.enableReaderMode(getActivity(), this::onTag, flags, null);
    }

    /** Stops waiting for a document. */
    @PluginMethod
    public void cancel(PluginCall call) {
        stop();
        if (pending != null) { pending.reject("Cancelled.", "cancelled"); release(); }
        call.resolve();
    }

    private void stop() {
        NfcAdapter adapter = NfcAdapter.getDefaultAdapter(getContext());
        if (adapter != null && getActivity() != null) adapter.disableReaderMode(getActivity());
    }

    private void release() {
        if (pending != null) { pending.setKeepAlive(false); pending.release(getBridge()); }
        pending = null;
        key = null;
        chipNonce = null;
    }

    private void onTag(Tag tag) {
        PluginCall call = pending;
        if (call == null) return;
        IsoDep iso = IsoDep.get(tag);
        if (iso == null) return;
        try {
            iso.setTimeout(10000);
            CardService card = CardService.getInstance(iso);
            card.open();
            PassportService service = new PassportService(card,
                PassportService.NORMAL_MAX_TRANCEIVE_LENGTH, PassportService.DEFAULT_MAX_BLOCKSIZE, false, false);
            service.open();
            boolean pace = false;
            try {
                CardAccessFile access = new CardAccessFile(
                    service.getInputStream(PassportService.EF_CARD_ACCESS, PassportService.DEFAULT_MAX_BLOCKSIZE));
                for (SecurityInfo info : access.getSecurityInfos()) {
                    if (info instanceof PACEInfo) {
                        PACEInfo p = (PACEInfo) info;
                        service.doPACE(key, p.getObjectIdentifier(), PACEInfo.toParameterSpec(p.getParameterId()), null);
                        pace = true;
                        break;
                    }
                }
            } catch (Exception noPace) {
                pace = false;
            }
            service.sendSelectApplet(pace);
            if (!pace) {
                if (!(key instanceof BACKey)) throw new IllegalStateException("This card needs PACE with its card access number.");
                service.doBAC(key);
            }
            JSObject out = new JSObject();
            out.put("pace", pace);
            out.put("sod", b64(bytes(service, PassportService.EF_SOD)));
            byte[] dg14 = optional(service, PassportService.EF_DG14);
            boolean chipAuthenticated = dg14 != null && chipAuthentication(service, dg14);
            // read under the session keys of chip authentication, if it ran:
            // only the chip itself can answer these
            out.put("dg1", b64(bytes(service, PassportService.EF_DG1)));
            out.put("dg2", b64(bytes(service, PassportService.EF_DG2)));
            if (dg14 != null) out.put("dg14", b64(dg14));
            byte[] dg15 = optional(service, PassportService.EF_DG15);
            if (dg15 != null) {
                out.put("dg15", b64(dg15));
                if (chipNonce != null && chipNonce.length == 8) {
                    PublicKey aaKey = new DG15File(new ByteArrayInputStream(dg15)).getPublicKey();
                    AAResult aa = service.doAA(aaKey, null, null, chipNonce);
                    out.put("active", b64(aa.getResponse()));
                }
            }
            out.put("chipAuthenticated", chipAuthenticated);
            stop();
            call.resolve(out);
            release();
        } catch (Exception e) {
            stop();
            String message = e.getMessage() == null ? e.getClass().getSimpleName() : e.getMessage();
            call.reject("The chip could not be read: " + message, "read-failed");
            release();
        } finally {
            try { iso.close(); } catch (Exception ignored) { }
        }
    }

    /**
     * Chip authentication (BSI TR-03110) with the key DG14 offers. Answers
     * whether it ran; a failure throws, so the read fails.
     */
    private static boolean chipAuthentication(PassportService service, byte[] dg14) throws Exception {
        ChipAuthenticationInfo info = null;
        ChipAuthenticationPublicKeyInfo publicKey = null;
        for (SecurityInfo si : new DG14File(new ByteArrayInputStream(dg14)).getSecurityInfos()) {
            if (si instanceof ChipAuthenticationInfo) info = (ChipAuthenticationInfo) si;
            else if (si instanceof ChipAuthenticationPublicKeyInfo) publicKey = (ChipAuthenticationPublicKeyInfo) si;
        }
        if (publicKey == null) return false;
        PublicKey pk = publicKey.getSubjectPublicKey();
        boolean ec = "EC".equals(pk.getAlgorithm()) || "ECDH".equals(pk.getAlgorithm());
        String oid = info != null ? info.getObjectIdentifier()
            : (ec ? SecurityInfo.ID_CA_ECDH_3DES_CBC_CBC : SecurityInfo.ID_CA_DH_3DES_CBC_CBC);
        BigInteger keyId = info != null && info.getKeyId() != null ? info.getKeyId() : publicKey.getKeyId();
        service.doEACCA(keyId, oid, publicKey.getObjectIdentifier(), pk);
        return true;
    }

    private static String b64(byte[] bytes) {
        return Base64.encodeToString(bytes, Base64.NO_WRAP);
    }

    private static byte[] bytes(PassportService service, short fid) throws Exception {
        try (InputStream in = service.getInputStream(fid, PassportService.DEFAULT_MAX_BLOCKSIZE)) {
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            byte[] buffer = new byte[4096];
            int n;
            while ((n = in.read(buffer)) > 0) out.write(buffer, 0, n);
            return out.toByteArray();
        }
    }

    private static byte[] optional(PassportService service, short fid) {
        try { return bytes(service, fid); } catch (Exception absent) { return null; }
    }
}
