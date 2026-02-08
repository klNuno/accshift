// CS2 Friend Code encoder
// Ported from github.com/emily33901/js-csfriendcode (reverse-engineered from CS2 binaries)

const ALPHABET = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

// --- Minimal MD5 (RFC 1321) ---

const S = [
  7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
  5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
  4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
  6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

const K = new Uint32Array(64);
for (let i = 0; i < 64; i++) {
  K[i] = Math.floor(Math.abs(Math.sin(i + 1)) * 0x100000000) >>> 0;
}

function md5(input: Uint8Array): Uint8Array {
  const msgLen = input.length;
  const bitLen = msgLen * 8;
  const padCount = ((55 - (msgLen % 64)) + 64) % 64;
  const totalLen = msgLen + 1 + padCount + 8;
  const padded = new Uint8Array(totalLen);
  padded.set(input);
  padded[msgLen] = 0x80;
  const dv = new DataView(padded.buffer);
  dv.setUint32(totalLen - 8, bitLen, true);

  let a0 = 0x67452301 >>> 0;
  let b0 = 0xefcdab89 >>> 0;
  let c0 = 0x98badcfe >>> 0;
  let d0 = 0x10325476 >>> 0;

  for (let off = 0; off < totalLen; off += 64) {
    const M = new Uint32Array(16);
    for (let j = 0; j < 16; j++) M[j] = dv.getUint32(off + j * 4, true);

    let A = a0, B = b0, C = c0, D = d0;
    for (let i = 0; i < 64; i++) {
      let F: number, g: number;
      if (i < 16) { F = (B & C) | (~B & D); g = i; }
      else if (i < 32) { F = (D & B) | (~D & C); g = (5 * i + 1) % 16; }
      else if (i < 48) { F = B ^ C ^ D; g = (3 * i + 5) % 16; }
      else { F = C ^ (B | ~D); g = (7 * i) % 16; }

      F = (F + A + K[i] + M[g]) >>> 0;
      A = D;
      D = C;
      C = B;
      B = (B + ((F << S[i]) | (F >>> (32 - S[i])))) >>> 0;
    }

    a0 = (a0 + A) >>> 0;
    b0 = (b0 + B) >>> 0;
    c0 = (c0 + C) >>> 0;
    d0 = (d0 + D) >>> 0;
  }

  const result = new Uint8Array(16);
  const rv = new DataView(result.buffer);
  rv.setUint32(0, a0, true);
  rv.setUint32(4, b0, true);
  rv.setUint32(8, c0, true);
  rv.setUint32(12, d0, true);
  return result;
}

// --- BigInt byte helpers ---

function toLEBytes(n: bigint): Uint8Array {
  const r = new Uint8Array(8);
  for (let i = 0; i < 8; i++) {
    r[i] = Number(n & 0xFFn);
    n >>= 8n;
  }
  return r;
}

function fromLEBytes(bytes: Uint8Array): bigint {
  let r = 0n;
  for (let i = bytes.length - 1; i >= 0; i--) {
    r = (r << 8n) | BigInt(bytes[i]);
  }
  return r;
}

// --- Friend Code ---

function hashSteamId(steamid: bigint): bigint {
  const accountId = steamid & 0xFFFFFFFFn;
  const strangeSteamId = accountId | 0x4353474F00000000n;
  const hash = md5(toLEBytes(strangeSteamId));
  return fromLEBytes(hash.slice(0, 4));
}

function makeU64(hi: bigint, lo: bigint): bigint {
  return (hi << 32n) | lo;
}

export function encodeFriendCode(steamId64: string): string {
  let steamid = BigInt(steamId64);
  const h = hashSteamId(steamid);

  let r = 0n;
  for (let i = 0; i < 8; i++) {
    const idNibble = steamid & 0xFn;
    steamid >>= 4n;
    const hashNibble = (h >> BigInt(i)) & 1n;

    const a = (r << 4n) | idNibble;
    r = makeU64(r >> 28n, a);
    r = makeU64(r >> 31n, (a << 1n) | hashNibble);
  }

  // Byte-swap: to LE bytes, reverse, read as LE
  const leBytes = toLEBytes(r);
  const reversed = new Uint8Array([...leBytes].reverse());
  let val = fromLEBytes(reversed);

  let code = "";
  for (let i = 0; i < 13; i++) {
    if (i === 4 || i === 9) {
      code += "-";
    }
    code += ALPHABET[Number(val & 0x1Fn)];
    val >>= 5n;
  }

  // Strip AAAA- prefix
  if (code.startsWith("AAAA-")) {
    return code.slice(5);
  }
  return code;
}
