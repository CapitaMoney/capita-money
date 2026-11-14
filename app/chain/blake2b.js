// SPDX-License-Identifier: AGPL-3.0-only
// BLAKE2b (RFC 7693), unkeyed, any output length 1..64 bytes. The chain's
// hash: account ids, challenges and call hashes are BLAKE2b-256
// (`capita_keys::encoding::blake2_256`, `sp_io::hashing::blake2_256`).
// Plain BigInt arithmetic: the app hashes a few hundred bytes per payment.

const MASK = (1n << 64n) - 1n;
const IV = [
  0x6a09e667f3bcc908n, 0xbb67ae8584caa73bn, 0x3c6ef372fe94f82bn, 0xa54ff53a5f1d36f1n,
  0x510e527fade682d1n, 0x9b05688c2b3e6c1fn, 0x1f83d9abfb41bd6bn, 0x5be0cd19137e2179n,
];
const SIGMA = [
  [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
  [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
  [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
  [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
  [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
  [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
  [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
  [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
  [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
  [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

const rotr = (x, n) => ((x >> n) | (x << (64n - n))) & MASK;

function mix(v, a, b, c, d, x, y) {
  v[a] = (v[a] + v[b] + x) & MASK;
  v[d] = rotr(v[d] ^ v[a], 32n);
  v[c] = (v[c] + v[d]) & MASK;
  v[b] = rotr(v[b] ^ v[c], 24n);
  v[a] = (v[a] + v[b] + y) & MASK;
  v[d] = rotr(v[d] ^ v[a], 16n);
  v[c] = (v[c] + v[d]) & MASK;
  v[b] = rotr(v[b] ^ v[c], 63n);
}

function word(block, i) {
  let w = 0n;
  for (let k = 7; k >= 0; k--) w = (w << 8n) | BigInt(block[i * 8 + k]);
  return w;
}

function compress(h, block, t, last) {
  const v = h.concat(IV);
  v[12] ^= t & MASK;
  v[13] ^= t >> 64n;
  if (last) v[14] ^= MASK;
  const m = Array.from({ length: 16 }, (_, i) => word(block, i));
  for (let r = 0; r < 12; r++) {
    const s = SIGMA[r % 10];
    mix(v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
    mix(v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
    mix(v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
    mix(v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
    mix(v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
    mix(v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
    mix(v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
    mix(v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
  }
  for (let i = 0; i < 8; i++) h[i] ^= v[i] ^ v[i + 8];
}

/** BLAKE2b of `data` (Uint8Array), `outlen` bytes (default 32). */
export function blake2b(data, outlen = 32) {
  if (!(outlen >= 1 && outlen <= 64)) throw new RangeError("outlen 1..64");
  const h = IV.slice();
  h[0] ^= 0x01010000n ^ BigInt(outlen);
  let t = 0n;
  let off = 0;
  while (data.length - off > 128) {
    t += 128n;
    compress(h, data.subarray(off, off + 128), t, false);
    off += 128;
  }
  const block = new Uint8Array(128);
  block.set(data.subarray(off));
  t += BigInt(data.length - off);
  compress(h, block, t, true);
  const out = new Uint8Array(64);
  for (let i = 0; i < 8; i++) for (let k = 0; k < 8; k++) out[i * 8 + k] = Number((h[i] >> BigInt(8 * k)) & 0xffn);
  return out.slice(0, outlen);
}
