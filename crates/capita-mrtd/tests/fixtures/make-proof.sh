#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# Makes the test chip that proves itself (ADR 0024): the same DG1 and DG2 as
# make.sh, plus DG14 (chip authentication key + active authentication info,
# ECDSA with SHA-256) and DG15 (the active authentication key), all four
# listed in EF.SOD, under a CSCA of its own. The active authentication key
# is kept (aa.key) so the tests can answer nonces like the chip. Run once
# from this folder after make.sh; the files are kept:
#   bash make-proof.sh
# Needs openssl and python3.
set -euo pipefail
cd "$(dirname "$0")"
out=proof
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

subj() { printf '/C=UT/O=Utopia test state/CN=%s' "$1"; }
openssl ecparam -name prime256v1 -genkey -noout -out "$tmp/csca.key"
openssl req -new -x509 -key "$tmp/csca.key" -subj "$(subj 'CSCA Utopia proof test')" -days 9000 -sha256 \
  -addext "basicConstraints=critical,CA:TRUE,pathlen:0" -addext "keyUsage=critical,keyCertSign,cRLSign" \
  -out "$out/csca.pem"
openssl ecparam -name prime256v1 -genkey -noout -out "$tmp/ds.key"
openssl req -new -key "$tmp/ds.key" -subj "$(subj 'DS Utopia proof test')" -out "$tmp/ds.csr"
printf 'keyUsage=critical,digitalSignature\n' > "$tmp/ds.ext"
openssl x509 -req -in "$tmp/ds.csr" -CA "$out/csca.pem" -CAkey "$tmp/csca.key" -CAcreateserial \
  -CAserial "$tmp/csca.srl" -days 8000 -sha256 -extfile "$tmp/ds.ext" -out "$tmp/ds.pem" 2>/dev/null
openssl ecparam -name prime256v1 -genkey -noout -out "$out/aa.key"
openssl ec -in "$out/aa.key" -pubout -outform DER -out "$tmp/aa.spki" 2>/dev/null
openssl ecparam -name prime256v1 -genkey -noout -out "$tmp/ca.key"
openssl ec -in "$tmp/ca.key" -pubout -outform DER -out "$tmp/ca.spki" 2>/dev/null

python3 -I - "$tmp" "$out" <<'PY'
import hashlib, sys, pathlib
tmp, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])

def tlv(tag: bytes, value: bytes) -> bytes:
    n = len(value)
    if n < 0x80: length = bytes([n])
    elif n < 0x100: length = bytes([0x81, n])
    else: length = bytes([0x82, n >> 8, n & 0xff])
    return tag + length + value

def oid(hexs): return tlv(b"\x06", bytes.fromhex(hexs))
def der_int(n): return tlv(b"\x02", n.to_bytes(max(1, (n.bit_length() + 8) // 8), "big"))

dg1 = pathlib.Path("dg1.bin").read_bytes()
dg2 = pathlib.Path("dg2.bin").read_bytes()
# ChipAuthenticationPublicKeyInfo (id-PK-ECDH) + ActiveAuthenticationInfo (ecdsa-with-SHA256)
ca_info = tlv(b"\x30", oid("04007f000702020102") + (tmp / "ca.spki").read_bytes())
aa_info = tlv(b"\x30", oid("678108010105") + der_int(1) + oid("2a8648ce3d040302"))
dg14 = tlv(b"\x6e", tlv(b"\x31", ca_info + aa_info))
dg15 = tlv(b"\x6f", (tmp / "aa.spki").read_bytes())

sha256 = tlv(b"\x30", oid("608648016503040201"))
hashes = tlv(b"\x30", b"".join(tlv(b"\x30", der_int(n) + tlv(b"\x04", hashlib.sha256(d).digest()))
                                for n, d in ((1, dg1), (2, dg2), (14, dg14), (15, dg15))))
(tmp / "lds.der").write_bytes(tlv(b"\x30", der_int(0) + sha256 + hashes))
(out / "dg14.bin").write_bytes(dg14)
(out / "dg15.bin").write_bytes(dg15)
PY

openssl cms -sign -binary -nodetach -nosmimecap -md sha256 -econtent_type 2.23.136.1.1.1 \
  -signer "$tmp/ds.pem" -inkey "$tmp/ds.key" -in "$tmp/lds.der" -outform DER -out "$tmp/sod.cms"
python3 -I - "$tmp" "$out" <<'PY'
import sys, pathlib
cms = (pathlib.Path(sys.argv[1]) / "sod.cms").read_bytes()
n = len(cms)
length = bytes([0x82, n >> 8, n & 0xff]) if n >= 0x100 else bytes([0x81, n])
(pathlib.Path(sys.argv[2]) / "sod.bin").write_bytes(b"\x77" + length + cms)
PY
ls -l "$out"
