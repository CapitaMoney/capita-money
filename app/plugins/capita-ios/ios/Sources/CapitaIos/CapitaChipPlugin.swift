// SPDX-License-Identifier: AGPL-3.0-only
// Capita Money – iPhone shell: reading the chip of a passport or identity
// card (ICAO 9303) for the document check (docs/adr/0021-verification-ways.md),
// with Core NFC and NFCPassportReader 3.0.0 (MIT). Same calls and answers as
// the Android shell's CapitaChipPlugin (app/native/chip.js).
//
// Reads EF.SOD, DG1 (machine-readable zone), DG2 (photo), DG14/DG15 where
// present, and lets the chip prove itself (ADR 0024): chip authentication
// where DG14 offers it (the files after it are read under the chip's new
// session keys), active authentication of the service's chip nonce where DG15
// exists. Keeps nothing; the bytes go to the identity service.
// PACE and BAC with the document number and dates; the card access number
// (CAN) cannot be used on iPhones (the library has no CAN path).

import Capacitor
import CoreNFC
import Foundation
import NFCPassportReader

@objc(CapitaChipPlugin)
public class CapitaChipPlugin: CAPPlugin, CAPBridgedPlugin {
    public let identifier = "CapitaChipPlugin"
    public let jsName = "CapitaChip"
    public let pluginMethods: [CAPPluginMethod] = [
        CAPPluginMethod(name: "status", returnType: CAPPluginReturnPromise),
        CAPPluginMethod(name: "read", returnType: CAPPluginReturnPromise),
        CAPPluginMethod(name: "cancel", returnType: CAPPluginReturnPromise)
    ]

    private var reading: Task<Void, Never>?
    private var pending: CAPPluginCall?

    /// iPhones cannot switch NFC off: a reader is either there or not.
    @objc func status(_ call: CAPPluginCall) {
        let nfc = NFCTagReaderSession.readingAvailable
        call.resolve(["nfc": nfc, "enabled": nfc])
    }

    /// ICAO 9303 check digit (weights 7, 3, 1; < = 0, A = 10 …).
    static func checkDigit(_ text: String) -> String {
        let weights = [7, 3, 1]
        var sum = 0
        for (i, c) in text.unicodeScalars.enumerated() {
            let v: Int
            switch c {
            case "0"..."9": v = Int(c.value) - 48
            case "A"..."Z": v = Int(c.value) - 55
            default: v = 0
            }
            sum += v * weights[i % 3]
        }
        return String(sum % 10)
    }

    /// The access key of BAC and PACE: number, birth and expiry, each with its check digit.
    static func mrzKey(number: String, birth: String, expiry: String) -> String {
        let n = number.count < 9 ? number + String(repeating: "<", count: 9 - number.count) : number
        return n + checkDigit(n) + birth + checkDigit(birth) + expiry + checkDigit(expiry)
    }

    @objc func read(_ call: CAPPluginCall) {
        guard NFCTagReaderSession.readingAvailable else {
            call.reject("This phone cannot read document chips.", "no-nfc"); return
        }
        if call.getString("can") != nil {
            call.reject("On an iPhone the chip opens with the document number and dates, not the card access number.", "no-can")
            return
        }
        guard let number = call.getString("documentNumber"), let birth = call.getString("birthDate"),
              let expiry = call.getString("expiryDate"), !number.isEmpty, birth.count == 6, expiry.count == 6 else {
            call.reject("Document number, date of birth and expiry are needed.", "no-key"); return
        }
        var challenge: [UInt8]?
        if let text = call.getString("chipNonce"), let nonce = Data(base64Encoded: text), nonce.count == 8 {
            challenge = [UInt8](nonce)
        }
        let key = Self.mrzKey(number: number, birth: birth, expiry: expiry)
        // state only on the main thread: calls arrive on the plugin's queue
        DispatchQueue.main.async { [self] in
            reading?.cancel()
            pending?.reject("Cancelled.", "cancelled")
            pending = call
            reading = Task { [weak self] in
                let reader = PassportReader()
                do {
                    let model = try await reader.readPassport(
                        mrzKey: key,
                        tags: [.COM, .SOD, .DG1, .DG2, .DG14, .DG15],
                        aaChallenge: challenge,
                        skipSecureElements: true,
                        skipCA: false,
                        skipPACE: false)
                    await MainActor.run { self?.finish(call, model: model, challenge: challenge) }
                } catch {
                    await MainActor.run { self?.fail(call, error: error) }
                }
            }
        }
    }

    private func finish(_ call: CAPPluginCall, model: NFCPassportModel, challenge: [UInt8]?) {
        guard pending === call else { return }
        pending = nil
        func b64(_ id: DataGroupId) -> String? {
            guard let dg = model.getDataGroup(id) else { return nil }
            return Data(dg.data).base64EncodedString()
        }
        guard let sod = b64(.SOD), let dg1 = b64(.DG1), let dg2 = b64(.DG2) else {
            call.reject("The chip could not be read: files missing", "read-failed"); return
        }
        var out: [String: Any] = [
            "pace": model.PACEStatus == .success,
            "sod": sod, "dg1": dg1, "dg2": dg2,
            "chipAuthenticated": model.chipAuthenticationStatus == .success
        ]
        if let dg14 = b64(.DG14) { out["dg14"] = dg14 }
        if let dg15 = b64(.DG15) {
            out["dg15"] = dg15
            if let challenge, model.activeAuthenticationChallenge == challenge,
               !model.activeAuthenticationSignature.isEmpty {
                out["active"] = Data(model.activeAuthenticationSignature).base64EncodedString()
            }
        }
        call.resolve(out)
    }

    private func fail(_ call: CAPPluginCall, error: Error) {
        guard pending === call else { return }
        pending = nil
        if let e = error as? NFCPassportReaderError, case .UserCanceled = e {
            call.reject("Cancelled.", "cancelled")
        } else {
            call.reject("The chip could not be read: \(error.localizedDescription)", "read-failed")
        }
    }

    /// Stops waiting: the call answers "cancelled". (While the system's NFC
    /// sheet is open the person cancels there.)
    @objc func cancel(_ call: CAPPluginCall) {
        DispatchQueue.main.async { [self] in
            reading?.cancel()
            reading = nil
            pending?.reject("Cancelled.", "cancelled")
            pending = nil
            call.resolve()
        }
    }
}
