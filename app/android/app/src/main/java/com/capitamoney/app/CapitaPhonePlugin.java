// SPDX-License-Identifier: AGPL-3.0-only
// Capita Money – Android shell: the phone check of a verification
// (docs/adr/0024-hardware-keys-checked-phones.md).
//
// For each verification a fresh P-256 session key is made in the Keystore
// with the identity service's nonce as attestation challenge. Its
// certificate chain – from the phone's security chip up to Google's
// hardware attestation root – says where the key lives, whether the phone's
// system is the one its maker signed, and which app (package + signing
// certificate) asked. The service checks it offline (crates/capita-attest);
// nothing is asked of Google, so phones without Google services work too.
// The session key then signs the request digest and is deleted.

package com.capitamoney.app;

import android.os.Build;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.security.keystore.StrongBoxUnavailableException;
import android.util.Base64;

import com.getcapacitor.JSObject;
import com.getcapacitor.Plugin;
import com.getcapacitor.PluginCall;
import com.getcapacitor.PluginMethod;
import com.getcapacitor.annotation.CapacitorPlugin;

import java.security.KeyPairGenerator;
import java.security.KeyStore;
import java.security.PrivateKey;
import java.security.Signature;
import java.security.spec.ECGenParameterSpec;

@CapacitorPlugin(name = "CapitaPhone")
public class CapitaPhonePlugin extends Plugin {

    static final String ALIAS = "capita-session";

    private static KeyGenParameterSpec spec(byte[] nonce, boolean inStrongBox) {
        KeyGenParameterSpec.Builder b = new KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_SIGN)
            .setAlgorithmParameterSpec(new ECGenParameterSpec("secp256r1"))
            .setDigests(KeyProperties.DIGEST_SHA256)
            .setAttestationChallenge(nonce);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) b.setIsStrongBoxBacked(inStrongBox);
        return b.build();
    }

    /** Makes the session key for the nonce (base64). Answers its certificate chain (DER, base64, key first). */
    @PluginMethod
    public void sessionKey(PluginCall call) {
        String nonce = call.getString("nonce");
        if (nonce == null) { call.reject("No nonce.", "no-nonce"); return; }
        byte[] challenge = Base64.decode(nonce, Base64.DEFAULT);
        try {
            KeyStore ks = CapitaKeyPlugin.keyStore();
            ks.deleteEntry(ALIAS);
            KeyPairGenerator g = KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, "AndroidKeyStore");
            boolean strong = Build.VERSION.SDK_INT >= Build.VERSION_CODES.P
                && getContext().getPackageManager().hasSystemFeature(android.content.pm.PackageManager.FEATURE_STRONGBOX_KEYSTORE);
            try {
                g.initialize(spec(challenge, strong));
                g.generateKeyPair();
            } catch (StrongBoxUnavailableException e) {
                g.initialize(spec(challenge, false));
                g.generateKeyPair();
            }
            JSObject out = new JSObject();
            out.put("chain", CapitaKeyPlugin.chain(ks, ALIAS));
            call.resolve(out);
        } catch (Exception e) {
            call.reject("This phone cannot prove that it is genuine: " + e.getMessage(), "no-attestation");
        }
    }

    /** Signs the request digest (base64) with the session key, then deletes the key. */
    @PluginMethod
    public void signSession(PluginCall call) {
        String digest = call.getString("digest");
        if (digest == null) { call.reject("Nothing to sign.", "no-digest"); return; }
        try {
            KeyStore ks = CapitaKeyPlugin.keyStore();
            PrivateKey key = (PrivateKey) ks.getKey(ALIAS, null);
            if (key == null) { call.reject("No session key; start again.", "no-session"); return; }
            Signature s = Signature.getInstance("SHA256withECDSA");
            s.initSign(key);
            s.update(Base64.decode(digest, Base64.DEFAULT));
            JSObject out = new JSObject();
            out.put("signature", CapitaKeyPlugin.b64(s.sign()));
            ks.deleteEntry(ALIAS);
            call.resolve(out);
        } catch (Exception e) {
            call.reject("Signing failed: " + e.getMessage(), "sign-failed");
        }
    }
}
