// SPDX-License-Identifier: AGPL-3.0-only
// xxHash64 and the "twox 128" of Substrate storage names: the first half of
// every storage address is twox128(pallet) ‖ twox128(item). Pure; tested
// against known addresses (app/tests/chain-read.test.mjs).

const M = (1n << 64n) - 1n;
const P1 = 11400714785074694791n;
const P2 = 14029467366897019727n;
const P3 = 1609587929392839161n;
const P4 = 9650029242287828579n;
const P5 = 2870177450012600261n;

const rotl = (x, r) => ((x << BigInt(r)) | (x >> BigInt(64 - r))) & M;
const round = (acc, lane) => (rotl((acc + lane * P2) & M, 31) * P1) & M;
const merge = (acc, v) => (((acc ^ round(0n, v)) * P1) + P4) & M;

function u64(b, i) {
  let v = 0n;
  for (let k = 7; k >= 0; k--) v = (v << 8n) | BigInt(b[i + k]);
  return v;
}
const u32 = (b, i) => BigInt(b[i] | (b[i + 1] << 8) | (b[i + 2] << 16)) + (BigInt(b[i + 3]) << 24n);

/** xxHash64 of `data` (bytes) with `seed`, as a BigInt. */
export function xxhash64(data, seed = 0n) {
  const len = data.length;
  let i = 0;
  let h;
  if (len >= 32) {
    let v1 = (seed + P1 + P2) & M;
    let v2 = (seed + P2) & M;
    let v3 = seed & M;
    let v4 = (seed - P1) & M;
    for (; i + 32 <= len; i += 32) {
      v1 = round(v1, u64(data, i));
      v2 = round(v2, u64(data, i + 8));
      v3 = round(v3, u64(data, i + 16));
      v4 = round(v4, u64(data, i + 24));
    }
    h = (rotl(v1, 1) + rotl(v2, 7) + rotl(v3, 12) + rotl(v4, 18)) & M;
    h = merge(merge(merge(merge(h, v1), v2), v3), v4);
  } else {
    h = (seed + P5) & M;
  }
  h = (h + BigInt(len)) & M;
  for (; i + 8 <= len; i += 8) h = ((rotl(h ^ round(0n, u64(data, i)), 27) * P1) + P4) & M;
  if (i + 4 <= len) {
    h = ((rotl(h ^ ((u32(data, i) * P1) & M), 23) * P2) + P3) & M;
    i += 4;
  }
  for (; i < len; i++) h = (rotl(h ^ ((BigInt(data[i]) * P5) & M), 11) * P1) & M;
  h ^= h >> 33n;
  h = (h * P2) & M;
  h ^= h >> 29n;
  h = (h * P3) & M;
  h ^= h >> 32n;
  return h;
}

const le8 = (v) => Uint8Array.from({ length: 8 }, (_, k) => Number((v >> BigInt(8 * k)) & 255n));

/** twox128: xxHash64 with seeds 0 and 1, each little-endian. */
export function twox128(data) {
  const out = new Uint8Array(16);
  out.set(le8(xxhash64(data, 0n)), 0);
  out.set(le8(xxhash64(data, 1n)), 8);
  return out;
}
