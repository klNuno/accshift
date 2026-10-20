//! CS2 friend codes, ported from `src/lib/platforms/steam/friendCode.ts`
//! (itself from github.com/emily33901/js-csfriendcode).

const ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

const S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// Minimal MD5 (RFC 1321), as the TS file carries its own.
pub fn md5(input: &[u8]) -> [u8; 16] {
    let k: Vec<u32> = (0..64)
        .map(|i| ((f64::from(i + 1)).sin().abs() * 4_294_967_296.0).floor() as u32)
        .collect();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut padded = input.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_le_bytes());

    let (mut a0, mut b0, mut c0, mut d0) = (
        0x6745_2301_u32,
        0xefcd_ab89_u32,
        0x98ba_dcfe_u32,
        0x1032_5476_u32,
    );
    for chunk in padded.chunks(64) {
        let m: Vec<u32> = chunk
            .chunks(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    out[..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..].copy_from_slice(&d0.to_le_bytes());
    out
}

/// `BigInt(text)` for the integers a Steam id can be: decimal, `0x`, `0o`,
/// `0b`, surrounding whitespace, the empty string as 0. `None` where BigInt
/// throws, and past i128 (BigInt has no ceiling; no Steam id comes close).
fn parse_big_int(text: &str) -> Option<i128> {
    let trimmed = crate::js::trim(text);
    if trimmed.is_empty() {
        return Some(0);
    }
    let lower = trimmed.to_ascii_lowercase();
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(digits) = lower.strip_prefix(prefix) {
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return None;
            }
            return i128::from_str_radix(digits, radix).ok();
        }
    }
    let (negative, digits) = match trimmed.as_bytes()[0] {
        b'-' => (true, &trimmed[1..]),
        b'+' => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value: i128 = digits.parse().ok()?;
    Some(if negative { -value } else { value })
}

fn hash_steam_id(steam_id: i128) -> u128 {
    let account_id = (steam_id & 0xffff_ffff) as u64;
    let strange = account_id | 0x4353_474f_0000_0000;
    let hash = md5(&strange.to_le_bytes());
    u128::from(u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]]))
}

fn make_u64(hi: u128, lo: u128) -> u128 {
    (hi << 32) | lo
}

/// `encodeFriendCode`: the CS2 friend code of a SteamID64, empty for an id
/// that is not an integer.
pub fn encode_friend_code(steam_id64: &str) -> String {
    let Some(mut steam_id) = parse_big_int(steam_id64) else {
        return String::new();
    };
    let hash = hash_steam_id(steam_id);
    let mut r: u128 = 0;
    for i in 0..8 {
        let id_nibble = (steam_id & 0xf) as u128;
        steam_id >>= 4;
        let hash_nibble = (hash >> i) & 1;
        let a = (r << 4) | id_nibble;
        r = make_u64(r >> 28, a);
        r = make_u64(r >> 31, (a << 1) | hash_nibble);
    }
    // Byte-swap the low 64 bits: to LE bytes, reverse, read as LE.
    let mut value = (r as u64).swap_bytes();
    let mut code = String::with_capacity(15);
    for i in 0..13 {
        if i == 4 || i == 9 {
            code.push('-');
        }
        code.push(char::from(ALPHABET[(value & 0x1f) as usize]));
        value >>= 5;
    }
    match code.strip_prefix("AAAA-") {
        Some(rest) => rest.to_string(),
        None => code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_matches_the_rfc_vectors() {
        let hex = |bytes: [u8; 16]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(hex(md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
    }
}
