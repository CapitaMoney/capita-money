// SPDX-License-Identifier: AGPL-3.0-only
// Capita Money – Android shell: the money key in the phone's security chip
// (docs/adr/0024-hardware-keys-checked-phones.md).
//
// One P-256 key, made by the Android Keystore inside StrongBox where the
// phone has it, else inside the trusted environment. It can only sign,
// never leave the chip, never be backed up or synced. Every signature asks
// for fingerprint, face or screen lock (Android 11 and later: per use;
// Android 9 and 10: within ten seconds of the confirmation). The chain
// checks its signatures as DeviceKey::Hardware (crates/capita-keys).

package com.capitamoney.app;

import android.content.pm.PackageManager;
import android.os.Build;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.security.keystore.StrongBoxUnavailableException;
import android.util.Base64;

import androidx.biometric.BiometricManager;
import androidx.biometric.BiometricPrompt;
import androidx.core.content.ContextCompat;
import androidx.fragment.app.FragmentActivity;

import com.getcapacitor.JSArray;
import com.getcapacitor.JSObject;
import com.getcapacitor.Plugin;
import com.getcapacitor.PluginCall;
import com.getcapacitor.PluginMethod;
import com.getcapacitor.annotation.CapacitorPlugin;

import java.math.BigInteger;
import java.security.KeyPairGenerator;
import java.security.KeyStore;
import java.security.PrivateKey;
import java.security.Signature;
import java.security.cert.Certificate;
import java.security.interfaces.ECPublicKey;
import java.security.spec.ECGenParameterSpec;

@CapacitorPlugin(name = "CapitaKey")
public class CapitaKeyPlugin extends Plugin {

    static final String ALIAS = "capita-money-key";
    static final int MIN_SDK = Build.VERSION_CODES.P;

    static KeyStore keyStore() throws Exception {
        KeyStore ks = KeyStore.getInstance("AndroidKeyStore");
        ks.load(null);
        return ks;
    }

    /** The compressed SEC1 form of a P-256 public key: 02/03 ‖ x. */
    static byte[] compressed(ECPublicKey key) {
        byte[] x = unsigned32(key.getW().getAffineX());
        byte[] out = new byte[33];
        out[0] = (byte) (key.getW().getAffineY().testBit(0) ? 3 : 2);
        System.arraycopy(x, 0, out, 1, 32);
        return out;
    }

    static byte[] unsigned32(BigInteger n) {
        byte[] raw = n.toByteArray();
        byte[] out = new byte[32];
        int copy = Math.min(raw.length, 32);
        System.arraycopy(raw, raw.length - copy, out, 32 - copy, copy);
        return out;
    }

    static String b64(byte[] bytes) {
        return Base64.encodeToString(bytes, Base64.NO_WRAP);
    }

    static JSArray chain(KeyStore ks, String alias) throws Exception {
        JSArray out = new JSArray();
        Certificate[] certs = ks.getCertificateChain(alias);
        if (certs != null) for (Certificate c : certs) out.put(b64(c.getEncoded()));
        return out;
    }

    private boolean strongBox() {
        return Build.VERSION.SDK_INT >= MIN_SDK
            && getContext().getPackageManager().hasSystemFeature(PackageManager.FEATURE_STRONGBOX_KEYSTORE);
    }

    @PluginMethod
    public void status(PluginCall call) {
        JSObject out = new JSObject();
        boolean possible = Build.VERSION.SDK_INT >= MIN_SDK;
        out.put("possible", possible);
        out.put("strongBox", strongBox());
        try {
            KeyStore ks = keyStore();
            boolean exists = possible && ks.containsAlias(ALIAS);
            out.put("exists", exists);
            if (exists) out.put("public", b64(compressed((ECPublicKey) ks.getCertificate(ALIAS).getPublicKey())));
            int can = BiometricManager.from(getContext()).canAuthenticate(authenticators());
            out.put("lock", can == BiometricManager.BIOMETRIC_SUCCESS);
        } catch (Exception e) {
            out.put("exists", false);
            out.put("lock", false);
        }
        call.resolve(out);
    }

    static int authenticators() {
        return Build.VERSION.SDK_INT >= Build.VERSION_CODES.R
            ? BiometricManager.Authenticators.BIOMETRIC_STRONG | BiometricManager.Authenticators.DEVICE_CREDENTIAL
            : BiometricManager.Authenticators.BIOMETRIC_WEAK | BiometricManager.Authenticators.DEVICE_CREDENTIAL;
    }

    private KeyGenParameterSpec spec(boolean inStrongBox, byte[] challenge) {
        KeyGenParameterSpec.Builder b = new KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_SIGN)
            .setAlgorithmParameterSpec(new ECGenParameterSpec("secp256r1"))
            .setDigests(KeyProperties.DIGEST_SHA256)
            .setUserAuthenticationRequired(true)
            // a new fingerprint must not lose the account
            .setInvalidatedByBiometricEnrollment(false)
            .setIsStrongBoxBacked(inStrongBox);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            b.setUserAuthenticationParameters(0,
                KeyProperties.AUTH_BIOMETRIC_STRONG | KeyProperties.AUTH_DEVICE_CREDENTIAL);
        } else {
            b.setUserAuthenticationValidityDurationSeconds(10);
        }
        if (challenge != null) b.setAttestationChallenge(challenge);
        return b.build();
    }

    /** Makes the money key. Answers its public key, where it lives, and its attestation chain. */
    @PluginMethod
    public void create(PluginCall call) {
        if (Build.VERSION.SDK_INT < MIN_SDK) { call.reject("This phone is too old for a key in its security chip.", "too-old"); return; }
        try {
            KeyStore ks = keyStore();
            if (ks.containsAlias(ALIAS)) { call.reject("This phone already has its key.", "exists"); return; }
            String challenge = call.getString("challenge");
            byte[] c = challenge == null ? null : Base64.decode(challenge, Base64.DEFAULT);
            KeyPairGenerator g = KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, "AndroidKeyStore");
            boolean inStrongBox = strongBox();
            try {
                g.initialize(spec(inStrongBox, c));
                g.generateKeyPair();
            } catch (StrongBoxUnavailableException e) {
                inStrongBox = false;
                g.initialize(spec(false, c));
                g.generateKeyPair();
            }
            JSObject out = new JSObject();
            out.put("public", b64(compressed((ECPublicKey) ks.getCertificate(ALIAS).getPublicKey())));
            out.put("strongBox", inStrongBox);
            out.put("chain", chain(ks, ALIAS));
            call.resolve(out);
        } catch (Exception e) {
            call.reject("The key could not be made: " + e.getMessage(), "create-failed");
        }
    }

    /**
     * Signs a 32-byte challenge (base64) after the person confirms with
     * fingerprint, face or screen lock. Answers the ECDSA signature (DER, base64).
     */
    @PluginMethod
    public void sign(PluginCall call) {
        String challenge = call.getString("challenge");
        if (challenge == null) { call.reject("Nothing to sign.", "no-challenge"); return; }
        final byte[] message = Base64.decode(challenge, Base64.DEFAULT);
        final String title = call.getString("title", "Confirm");
        final String subtitle = call.getString("subtitle", "");
        final FragmentActivity activity = getActivity();
        try {
            PrivateKey key = (PrivateKey) keyStore().getKey(ALIAS, null);
            if (key == null) { call.reject("This phone has no key.", "no-key"); return; }
            final Signature signature = Signature.getInstance("SHA256withECDSA");
            final boolean perUse = Build.VERSION.SDK_INT >= Build.VERSION_CODES.R;
            if (perUse) signature.initSign(key);
            final PrivateKey later = key;
            BiometricPrompt.AuthenticationCallback callback = new BiometricPrompt.AuthenticationCallback() {
                @Override
                public void onAuthenticationSucceeded(BiometricPrompt.AuthenticationResult result) {
                    try {
                        Signature s = perUse ? result.getCryptoObject().getSignature() : signature;
                        if (!perUse) s.initSign(later);
                        s.update(message);
                        JSObject out = new JSObject();
                        out.put("signature", b64(s.sign()));
                        call.resolve(out);
                    } catch (Exception e) {
                        call.reject("Signing failed: " + e.getMessage(), "sign-failed");
                    }
                }

                @Override
                public void onAuthenticationError(int code, CharSequence text) {
                    call.reject(text.toString(), "cancelled");
                }
            };
            activity.runOnUiThread(() -> {
                BiometricPrompt prompt = new BiometricPrompt(activity, ContextCompat.getMainExecutor(getContext()), callback);
                BiometricPrompt.PromptInfo.Builder info = new BiometricPrompt.PromptInfo.Builder()
                    .setTitle(title)
                    .setAllowedAuthenticators(authenticators());
                if (!subtitle.isEmpty()) info.setSubtitle(subtitle);
                if (perUse) prompt.authenticate(info.build(), new BiometricPrompt.CryptoObject(signature));
                else prompt.authenticate(info.build());
            });
        } catch (Exception e) {
            call.reject("Signing failed: " + e.getMessage(), "sign-failed");
        }
    }

    /** Deletes the money key (the app says first what that means: verify again or use a linked device). */
    @PluginMethod
    public void remove(PluginCall call) {
        try {
            keyStore().deleteEntry(ALIAS);
            call.resolve();
        } catch (Exception e) {
            call.reject("The key could not be removed: " + e.getMessage(), "remove-failed");
        }
    }
}
