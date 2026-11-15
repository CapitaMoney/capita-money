// SPDX-License-Identifier: AGPL-3.0-only
// Capita Money – iPhone shell: the money key inside the Secure Enclave
// (docs/adr/0024-hardware-keys-checked-phones.md). P-256, signing only; the
// key never leaves the chip, is in no backup and syncs nowhere. Each use asks
// for Face ID, Touch ID or the phone's code. Same calls as the Android
// shell's CapitaKeyPlugin: status, create, sign, remove (app/chain/keys.js).

import Capacitor
import CryptoKit
import Foundation
import LocalAuthentication
import Security

@objc(CapitaKeyPlugin)
public class CapitaKeyPlugin: CAPPlugin, CAPBridgedPlugin {
    public let identifier = "CapitaKeyPlugin"
    public let jsName = "CapitaKey"
    public let pluginMethods: [CAPPluginMethod] = [
        CAPPluginMethod(name: "status", returnType: CAPPluginReturnPromise),
        CAPPluginMethod(name: "create", returnType: CAPPluginReturnPromise),
        CAPPluginMethod(name: "sign", returnType: CAPPluginReturnPromise),
        CAPPluginMethod(name: "remove", returnType: CAPPluginReturnPromise)
    ]

    /// The one money key of this app on this phone.
    static let tag = Data("com.capitamoney.app.money".utf8)

    static func query(_ context: LAContext? = nil) -> [String: Any] {
        var q: [String: Any] = [
            kSecClass as String: kSecClassKey,
            kSecAttrApplicationTag as String: tag,
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
            kSecReturnRef as String: true
        ]
        if let context { q[kSecUseAuthenticationContext as String] = context }
        return q
    }

    /// The private key's handle (not the key: that stays in the chip).
    static func key(_ context: LAContext? = nil) -> SecKey? {
        var item: CFTypeRef?
        guard SecItemCopyMatching(query(context) as CFDictionary, &item) == errSecSuccess,
              let item, CFGetTypeID(item) == SecKeyGetTypeID() else { return nil }
        return (item as! SecKey)
    }

    /// The public key as a compressed P-256 point (33 bytes), as on chain.
    static func compressed(_ key: SecKey) -> Data? {
        guard let pub = SecKeyCopyPublicKey(key),
              let raw = SecKeyCopyExternalRepresentation(pub, nil) as Data?,
              raw.count == 65, raw.first == 0x04, let last = raw.last else { return nil }
        return Data([0x02 | (last & 1)]) + raw.subdata(in: 1..<33)
    }

    static func lockSet() -> Bool {
        LAContext().canEvaluatePolicy(.deviceOwnerAuthentication, error: nil)
    }

    @objc func status(_ call: CAPPluginCall) {
        var out: [String: Any] = [
            "possible": SecureEnclave.isAvailable,
            // The Secure Enclave is a chip of its own, like Android's StrongBox.
            "strongBox": SecureEnclave.isAvailable,
            "lock": Self.lockSet(),
            "exists": false
        ]
        if let key = Self.key(), let pub = Self.compressed(key) {
            out["exists"] = true
            out["public"] = pub.base64EncodedString()
        }
        call.resolve(out)
    }

    @objc func create(_ call: CAPPluginCall) {
        guard SecureEnclave.isAvailable else {
            call.reject("This phone has no security chip for keys.", "too-old"); return
        }
        guard Self.key() == nil else { call.reject("This phone already has its key.", "exists"); return }
        guard Self.lockSet() else {
            call.reject("Set a code for this phone first: the key asks for it.", "no-lock"); return
        }
        var error: Unmanaged<CFError>?
        // userPresence: Face ID, Touch ID or the code; a new face or finger
        // does not lose the key (else adding a finger would lose the account).
        guard let access = SecAccessControlCreateWithFlags(
            kCFAllocatorDefault, kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
            [.privateKeyUsage, .userPresence], &error) else {
            call.reject("The key could not be made: \(String(describing: error?.takeRetainedValue()))", "create-failed")
            return
        }
        let attributes: [String: Any] = [
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrKeySizeInBits as String: 256,
            kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
            kSecPrivateKeyAttrs as String: [
                kSecAttrIsPermanent as String: true,
                kSecAttrApplicationTag as String: Self.tag,
                kSecAttrAccessControl as String: access
            ] as [String: Any]
        ]
        guard let key = SecKeyCreateRandomKey(attributes as CFDictionary, &error),
              let pub = Self.compressed(key) else {
            call.reject("The key could not be made: \(String(describing: error?.takeRetainedValue()))", "create-failed")
            return
        }
        call.resolve(["public": pub.base64EncodedString(), "strongBox": true])
    }

    /// Signs the challenge (ECDSA P-256 over SHA-256 of it); answers DER, as Android does.
    @objc func sign(_ call: CAPPluginCall) {
        guard let text = call.getString("challenge"), let challenge = Data(base64Encoded: text) else {
            call.reject("Nothing to sign.", "no-challenge"); return
        }
        let title = call.getString("title") ?? "Confirm"
        let subtitle = call.getString("subtitle") ?? ""
        DispatchQueue.global(qos: .userInitiated).async {
            let context = LAContext()
            context.localizedReason = subtitle.isEmpty ? title : "\(title): \(subtitle)"
            guard let key = Self.key(context) else { call.reject("This phone has no key.", "no-key"); return }
            var error: Unmanaged<CFError>?
            guard let signature = SecKeyCreateSignature(
                key, .ecdsaSignatureMessageX962SHA256, challenge as CFData, &error) as Data? else {
                let e = error?.takeRetainedValue()
                let code = (e as Error?).map { ($0 as NSError).code } ?? 0
                if code == LAError.userCancel.rawValue || code == LAError.appCancel.rawValue
                    || code == LAError.systemCancel.rawValue || code == Int(errSecUserCanceled) {
                    call.reject("Cancelled.", "cancelled")
                } else {
                    call.reject("Signing failed: \(String(describing: e))", "sign-failed")
                }
                return
            }
            call.resolve(["signature": signature.base64EncodedString()])
        }
    }

    @objc func remove(_ call: CAPPluginCall) {
        var q = Self.query()
        q.removeValue(forKey: kSecReturnRef as String)
        let status = SecItemDelete(q as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            call.reject("The key could not be removed: \(status)", "remove-failed"); return
        }
        call.resolve()
    }
}
