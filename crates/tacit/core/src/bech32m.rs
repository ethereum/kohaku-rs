//! bech32m without BIP-173's 90-character cap: a pool address carries 97 bytes.

const CHARSET: &[u8; 32] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
const BECH32M: u32 = 0x2bc8_30a3;
const GENERATOR: [u32; 5] = [
    0x3b6a_57b2,
    0x2650_8e6d,
    0x1ea1_19fa,
    0x3d42_33dd,
    0x2a14_62b3,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Bech32Error {
    #[error("mixed-case address")]
    MixedCase,
    #[error("malformed address")]
    Malformed,
    #[error("invalid address character")]
    Character,
    #[error("invalid address checksum")]
    Checksum,
    #[error("invalid address padding")]
    Padding,
}

fn polymod(values: impl IntoIterator<Item = u8>) -> u32 {
    let mut chk: u32 = 1;
    for value in values {
        let top = chk >> 25;
        chk = ((chk & 0x01ff_ffff) << 5) ^ u32::from(value);
        for (i, g) in GENERATOR.iter().enumerate() {
            if (top >> i) & 1 == 1 {
                chk ^= g;
            }
        }
    }
    chk
}

fn expand_hrp(hrp: &str) -> impl Iterator<Item = u8> + '_ {
    hrp.bytes()
        .map(|c| c >> 5)
        .chain(std::iter::once(0))
        .chain(hrp.bytes().map(|c| c & 31))
}

fn convert_bits(data: &[u8], from: u32, to: u32, pad: bool) -> Result<Vec<u8>, Bech32Error> {
    let mut out = Vec::new();
    let max = (1u32 << to) - 1;
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &value in data {
        acc = (acc << from) | u32::from(value);
        bits += from;
        while bits >= to {
            bits -= to;
            out.push(((acc >> bits) & max) as u8);
        }
        acc &= (1 << bits) - 1;
    }
    if pad {
        if bits > 0 {
            out.push(((acc << (to - bits)) & max) as u8);
        }
    } else if bits >= from || (acc << (to - bits)) & max != 0 {
        return Err(Bech32Error::Padding);
    }
    Ok(out)
}

/// Encodes `bytes` under the human-readable part `hrp` (lowercase ASCII).
pub(crate) fn encode(hrp: &str, bytes: &[u8]) -> String {
    let words = convert_bits(bytes, 8, 5, true).expect("padding never fails");
    let check = polymod(expand_hrp(hrp).chain(words.iter().copied()).chain([0; 6])) ^ BECH32M;
    let mut out = String::with_capacity(hrp.len() + 1 + words.len() + 6);
    out.push_str(hrp);
    out.push('1');
    for w in words
        .iter()
        .copied()
        .chain((0..6).map(|i| ((check >> (5 * (5 - i))) & 31) as u8))
    {
        out.push(char::from(CHARSET[w as usize]));
    }
    out
}

/// Decodes a bech32m string into its human-readable part and bytes.
pub(crate) fn decode(text: &str) -> Result<(String, Vec<u8>), Bech32Error> {
    if text != text.to_lowercase() && text != text.to_uppercase() {
        return Err(Bech32Error::MixedCase);
    }
    let s = text.to_lowercase();
    let pos = s.rfind('1').ok_or(Bech32Error::Malformed)?;
    if pos < 1 || pos + 7 > s.len() {
        return Err(Bech32Error::Malformed);
    }
    let hrp = &s[..pos];
    let data = s[pos + 1..]
        .bytes()
        .map(|c| {
            CHARSET
                .iter()
                .position(|x| *x == c)
                .map(|i| i as u8)
                .ok_or(Bech32Error::Character)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if polymod(expand_hrp(hrp).chain(data.iter().copied())) != BECH32M {
        return Err(Bech32Error::Checksum);
    }
    let bytes = convert_bits(&data[..data.len() - 6], 5, 8, false)?;
    Ok((hrp.to_string(), bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_long_payloads() {
        let bytes: Vec<u8> = (0..97u8).collect();
        let text = encode("bp", &bytes);
        assert_eq!(decode(&text), Ok(("bp".to_string(), bytes.clone())));
        assert_eq!(decode(&text.to_uppercase()), Ok(("bp".to_string(), bytes)));
    }

    #[test]
    fn refuses_damage() {
        let text = encode("bp", &[1, 2, 3, 4, 5]);
        let last = text.chars().last().unwrap();
        let flipped = format!(
            "{}{}",
            &text[..text.len() - 1],
            if last == 'q' { 'p' } else { 'q' }
        );
        assert_eq!(decode(&flipped), Err(Bech32Error::Checksum));
        let mixed = format!("{}{}", &text[..4], text[4..].to_uppercase());
        assert_eq!(decode(&mixed), Err(Bech32Error::MixedCase));
        assert_eq!(decode("bp1"), Err(Bech32Error::Malformed));
    }
}
