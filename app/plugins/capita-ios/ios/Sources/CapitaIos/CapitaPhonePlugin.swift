// SPDX-License-Identifier: AGPL-3.0-only
// Capita Money – iPhone shell: the phone check of a verification with Apple's
// App Attest (docs/adr/0024-hardware-keys-checked-phones.md). For each
// verification a fresh App Attest key: Apple certifies it for
// SHA-256(nonce), then it signs the request digest
// (`capita_attest::apple::check`, app/native/phone.js). Needs the Apple
// developer team id in the identity service's list; without it the service
// refuses iPhone checks and iPhone people verify at a helper.

import Capacitor
import CryptoKit
import DeviceCheck
import Foundation

@objc(CapitaPhonePlugin)
public class CapitaPhonePlugin: CAPPlugin, CAPBridgedPlugin {
    public let identifier = "CapitaPhonePlugin"
    public let jsName = "CapitaPhone"
    public let pluginMethods: [CAPPluginMethod] = [
        CAPPluginMethod(name: "attest", returnType: CAPPluginReturnPromise),
        CAPPluginMethod(name: "assert", returnType: CAPPluginReturnPromise)
    ]

    /// A fresh key, certified by Apple for SHA-256(nonce). Answers keyId and attestation (base64).
    @objc func attest(_ call: CAPPluginCall) {
        guard let text = call.getString("nonce"), let nonce = Data(base64Encoded: text) else {
            call.reject("No nonce.", "no-nonce"); return
        }
        let service = DCAppAttestService.shared
        guard service.isSupported else {
            call.reject("This phone cannot prove that it is genuine.", "no-attestation"); return
        }
        Task {
            do {
                let keyId = try await service.generateKey()
                let attestation = try await service.attestKey(keyId, clientDataHash: Data(SHA256.hash(data: nonce)))
                call.resolve(["keyId": keyId, "attestation": attestation.base64EncodedString()])
            } catch {
                call.reject("This phone cannot prove that it is genuine: \(error.localizedDescription)", "no-attestation")
            }
        }
    }

    /// The key of `attest` signs the request digest: assertion over SHA-256(digest).
    @objc func assert(_ call: CAPPluginCall) {
        guard let keyId = call.getString("keyId") else { call.reject("No session key; start again.", "no-session"); return }
        guard let text = call.getString("digest"), let digest = Data(base64Encoded: text) else {
            call.reject("Nothing to sign.", "no-digest"); return
        }
        Task {
            do {
                let assertion = try await DCAppAttestService.shared.generateAssertion(
                    keyId, clientDataHash: Data(SHA256.hash(data: digest)))
                call.resolve(["assertion": assertion.base64EncodedString()])
            } catch {
                call.reject("Signing failed: \(error.localizedDescription)", "sign-failed")
            }
        }
    }
}
