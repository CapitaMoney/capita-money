# Roots of the phone check (ADR 0024)

The identity service checks phones offline against these public certificates. Fetched 2026-11-15.

| File | What | Source | SHA-256 of the certificate |
|---|---|---|---|
| `google/google-hardware-attestation-root-rsa.pem` | Google hardware attestation root, RSA 4096 (issue of 2022; the issues of 2016, 2019 and 2021 carry the same key, the check compares keys) | [developer.android.com – key attestation](https://developer.android.com/privacy-and-security/security-key-attestation) | `CE:DB:1C:B6:DC:89:6A:E5:EC:79:73:48:BC:E9:28:67:53:C2:B3:8E:E7:1C:E0:FB:E3:4A:9A:12:48:80:0D:FC` |
| `google/google-key-attestation-ca1.pem` | Google "Key Attestation CA1", ECDSA root of 2025 | same page | `6D:9D:B4:CE:6C:5C:0B:29:31:66:D0:89:86:E0:57:74:A8:77:6C:EB:52:5D:9E:43:29:52:0D:E1:2B:A4:BC:C0` |
| `apple-app-attestation-root-ca.pem` | Apple App Attestation Root CA | <https://www.apple.com/certificateauthority/Apple_App_Attestation_Root_CA.pem> | `1C:B9:82:3B:A2:8B:A6:AD:2D:33:A0:06:94:1D:E2:AE:4F:51:3E:F1:D4:E8:31:B9:F7:E0:FA:7B:62:42:C9:32` |

Revoked attestation certificates: Google's status list, <https://android.googleapis.com/attestation/status>. The operator fetches it (for example daily) and names the file as `android_revoked`; the phone never contacts Google:

```sh
curl -sSf https://android.googleapis.com/attestation/status -o trust/attestation/google-status.json
```

Check a fingerprint: `openssl x509 -in <file> -noout -fingerprint -sha256`.
