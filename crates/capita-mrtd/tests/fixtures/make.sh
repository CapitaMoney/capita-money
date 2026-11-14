#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# Makes the test chip of capita-mrtd: a country signing certificate (CSCA)
# of the fictional state Utopia, a document signer under it, DG1 with the
# ICAO 9303 specimen (Anna Maria Eriksson), DG2 with a small grey picture,
# and EF.SOD signed by the document signer. A second CSCA that signed
# nothing. Run once from this folder; the files are kept:
#   bash make.sh
# Needs openssl, python3 and ImageMagick (convert).
set -euo pipefail
cd "$(dirname "$0")"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

subj() { printf '/C=UT/O=Utopia test state/CN=%s' "$1"; }
openssl ecparam -name prime256v1 -genkey -noout -out "$tmp/csca.key"
openssl req -new -x509 -key "$tmp/csca.key" -subj "$(subj 'CSCA Utopia test')" -days 9000 -sha256 \
  -addext "basicConstraints=critical,CA:TRUE,pathlen:0" -addext "keyUsage=critical,keyCertSign,cRLSign" \
  -out csca.pem
openssl ecparam -name prime256v1 -genkey -noout -out "$tmp/other.key"
openssl req -new -x509 -key "$tmp/other.key" -subj "$(subj 'CSCA other test')" -days 9000 -sha256 \
  -addext "basicConstraints=critical,CA:TRUE,pathlen:0" -addext "keyUsage=critical,keyCertSign,cRLSign" \
  -out other-csca.pem
openssl ecparam -name prime256v1 -genkey -noout -out "$tmp/ds.key"
openssl req -new -key "$tmp/ds.key" -subj "$(subj 'DS Utopia test')" -out "$tmp/ds.csr"
printf 'keyUsage=critical,digitalSignature\n' > "$tmp/ds.ext"
openssl x509 -req -in "$tmp/ds.csr" -CA csca.pem -CAkey "$tmp/csca.key" -CAcreateserial -days 8000 -sha256 \
  -extfile "$tmp/ds.ext" -out "$tmp/ds.pem" 2>/dev/null
convert -size 24x32 xc:gray60 -quality 80 "$tmp/face.jpg"

python3 -I - "$tmp" <<'PY'
import hashlib, sys, pathlib
tmp = pathlib.Path(sys.argv[1])

def tlv(tag: bytes, value: bytes) -> bytes:
    n = len(value)
    if n < 0x80: length = bytes([n])
    elif n < 0x100: length = bytes([0x81, n])
    else: length = bytes([0x82, n >> 8, n & 0xff])
    return tag + length + value

mrz = ("P<UTOERIKSSON<<ANNA<MARIA<<<<<<<<<<<<<<<<<<<"
       "L898902C36UTO7408122F1204159ZE184226B<<<<<10").encode()
assert len(mrz) == 88
dg1 = tlv(b"\x61", tlv(b"\x5f\x1f", mrz))

jpeg = (tmp / "face.jpg").read_bytes()
# ISO 19794-5 facial record: header, one face, no feature points
face = (b"FAC\x00" + b"010\x00" + (14 + 20 + 12 + len(jpeg)).to_bytes(4, "big") + (1).to_bytes(2, "big")
        + (20 + 12 + len(jpeg)).to_bytes(4, "big") + b"\x00\x00" + b"\x00" * 14
        + b"\x01\x00" + (24).to_bytes(2, "big") + (32).to_bytes(2, "big") + b"\x01\x00\x00\x00\x00\x00"
        + jpeg)
header = tlv(b"\xa1", tlv(b"\x80", b"\x01\x01") + tlv(b"\x87", b"\x01\x01") + tlv(b"\x88", b"\x00\x08"))
dg2 = tlv(b"\x75", tlv(b"\x7f\x61", tlv(b"\x02", b"\x01") + tlv(b"\x7f\x60", header + tlv(b"\x5f\x2e", face))))

def der_int(n): return tlv(b"\x02", n.to_bytes(max(1, (n.bit_length() + 8) // 8), "big"))
sha256 = tlv(b"\x30", tlv(b"\x06", bytes.fromhex("608648016503040201")))
hashes = tlv(b"\x30", b"".join(tlv(b"\x30", der_int(n) + tlv(b"\x04", hashlib.sha256(d).digest()))
                                for n, d in ((1, dg1), (2, dg2))))
lds = tlv(b"\x30", der_int(0) + sha256 + hashes)
(tmp / "lds.der").write_bytes(lds)
pathlib.Path("dg1.bin").write_bytes(dg1)
pathlib.Path("dg2.bin").write_bytes(dg2)
PY

openssl cms -sign -binary -nodetach -nosmimecap -md sha256 -econtent_type 2.23.136.1.1.1 \
  -signer "$tmp/ds.pem" -inkey "$tmp/ds.key" -in "$tmp/lds.der" -outform DER -out "$tmp/sod.cms"
python3 -I - "$tmp" <<'PY'
import sys, pathlib
cms = (pathlib.Path(sys.argv[1]) / "sod.cms").read_bytes()
n = len(cms)
length = bytes([0x82, n >> 8, n & 0xff]) if n >= 0x100 else bytes([0x81, n])
pathlib.Path("sod.bin").write_bytes(b"\x77" + length + cms)
PY
ls -l csca.pem other-csca.pem dg1.bin dg2.bin sod.bin
