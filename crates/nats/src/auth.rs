//! Signing in with keys: NKeys (an Ed25519 seed, written `SU...`) and the
//! `.creds` files `nsc` and `nats` write (a user's JWT, then its seed).
//!
//! A server that wants a key sends a nonce in its `INFO`; the client signs
//! it with its seed and sends the signature, and its JWT or public key, in
//! `CONNECT` ([`crate::Connect::nkey`]).

use crate::b64;

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// The first byte of a seed's raw form, before its type's top bits.
const PREFIX_SEED: u8 = 18 << 3;
/// What a key is for: a user's.
pub const PREFIX_USER: u8 = 20 << 3;
pub const PREFIX_ACCOUNT: u8 = 0;
pub const PREFIX_OPERATOR: u8 = 14 << 3;
pub const PREFIX_SERVER: u8 = 13 << 3;

/// Base32 (RFC 4648), no padding.
pub fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &b in bytes {
        acc = acc << 8 | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[(acc >> bits & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[(acc << (5 - bits) & 31) as usize] as char);
    }
    out
}

pub fn base32_decode(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for c in text.trim().trim_end_matches('=').bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a',
            b'2'..=b'7' => c - b'2' + 26,
            _ => return Err(format!("{:?} isn't base32", c as char)),
        };
        acc = (acc << 5 | v as u32) & 0xffff;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

/// CRC-16/XMODEM, what an NKey ends with (little-endian).
pub fn crc16(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in bytes {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { crc << 1 ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

fn with_crc(mut raw: Vec<u8>) -> String {
    let crc = crc16(&raw);
    raw.extend_from_slice(&crc.to_le_bytes());
    base32_encode(&raw)
}

fn checked(text: &str) -> Result<Vec<u8>, String> {
    let raw = base32_decode(text)?;
    if raw.len() < 4 {
        return Err("too short for an NKey".into());
    }
    let (body, crc) = raw.split_at(raw.len() - 2);
    if crc16(body).to_le_bytes() != crc {
        return Err("its checksum is wrong: not an NKey, or mistyped".into());
    }
    Ok(body.to_vec())
}

/// A seed, written as NKeys are, for a key of `prefix`'s kind.
pub fn encode_seed(prefix: u8, seed: &[u8; 32]) -> String {
    let mut raw = vec![PREFIX_SEED | prefix >> 5, (prefix & 31) << 3];
    raw.extend_from_slice(seed);
    with_crc(raw)
}

/// A public key, written as NKeys are: `U...` for a user's.
pub fn encode_public(prefix: u8, key: &[u8; 32]) -> String {
    let mut raw = vec![prefix];
    raw.extend_from_slice(key);
    with_crc(raw)
}

/// An Ed25519 key pair from an NKey seed.
#[derive(Clone)]
pub struct KeyPair {
    prefix: u8,
    pair: ed25519_compact::KeyPair,
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KeyPair({})", self.public_key())
    }
}

impl KeyPair {
    /// From a seed, `SU...` (a user's) or any other kind's.
    pub fn from_seed(seed: &str) -> Result<KeyPair, String> {
        let raw = checked(seed.trim())?;
        if raw.len() != 34 || raw[0] & 0xf8 != PREFIX_SEED {
            return Err("not an NKey seed (one starts with S)".into());
        }
        let prefix = (raw[0] & 7) << 5 | (raw[1] & 0xf8) >> 3;
        let bytes: [u8; 32] = raw[2..].try_into().expect("34 bytes");
        Ok(KeyPair::from_raw(prefix, bytes))
    }

    pub fn from_raw(prefix: u8, seed: [u8; 32]) -> KeyPair {
        KeyPair { prefix, pair: ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new(seed)) }
    }

    /// Its public key, as NKeys write it.
    pub fn public_key(&self) -> String {
        encode_public(self.prefix, &self.pair.pk)
    }

    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        *self.pair.sk.sign(message, None)
    }

    /// The server's nonce signed, as `CONNECT`'s `sig` takes it.
    pub fn sign_nonce(&self, nonce: &str) -> String {
        b64::encode_url(&self.sign(nonce.as_bytes()))
    }

    pub fn verify(&self, message: &[u8], signature: &[u8; 64]) -> bool {
        self.pair.pk.verify(message, &ed25519_compact::Signature::new(*signature)).is_ok()
    }
}

/// What a `.creds` file holds.
#[derive(Debug, Clone)]
pub struct Creds {
    pub jwt: String,
    pub key: KeyPair,
}

/// Reads a `.creds` file: the blocks between its `-----BEGIN ...-----` and
/// `------END ...------` lines, the JWT first, then the seed.
pub fn parse_creds(text: &str) -> Result<Creds, String> {
    let mut blocks = Vec::new();
    let mut lines = text.lines().map(str::trim);
    while let Some(line) = lines.next() {
        if line.starts_with("---") && line.contains("BEGIN") {
            let body: String = lines.by_ref().take_while(|l| !l.starts_with("---")).collect::<Vec<_>>().concat();
            if !body.is_empty() {
                blocks.push(body);
            }
        }
    }
    let jwt = blocks.first().ok_or("no JWT in it: a .creds file has a JWT, then a seed")?;
    let seed = blocks.iter().skip(1).find(|b| b.starts_with('S')).ok_or("no seed in it: a .creds file has a JWT, then a seed")?;
    if jwt.split('.').count() != 3 {
        return Err("its first block isn't a JWT".into());
    }
    Ok(Creds { jwt: jwt.clone(), key: KeyPair::from_seed(seed)? })
}

/// A JWT's claims, unchecked: its `name`, its `sub`ject, when it ends.
pub fn jwt_claims(jwt: &str) -> Option<serde_json::Value> {
    let payload = jwt.split('.').nth(1)?;
    serde_json::from_slice(&b64::decode(payload).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn crc16_is_xmodem() {
        assert_eq!(crc16(b"123456789"), 0x31c3);
    }

    #[test]
    fn base32_round_trips() {
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32_decode("MZXW6YTBOI").unwrap(), b"foobar");
        let all: Vec<u8> = (0..=255).collect();
        for n in 0..all.len() {
            assert_eq!(base32_decode(&base32_encode(&all[..n])).unwrap(), &all[..n]);
        }
    }

    #[test]
    fn ed25519_signs_as_rfc_8032_says() {
        let seed: [u8; 32] = hex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60").try_into().unwrap();
        let key = KeyPair::from_raw(PREFIX_USER, seed);
        let public: [u8; 32] = hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a").try_into().unwrap();
        assert_eq!(key.public_key(), encode_public(PREFIX_USER, &public));
        let sig = key.sign(b"");
        assert_eq!(sig.to_vec(), hex("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"));
        assert!(key.verify(b"", &sig));
    }

    #[test]
    fn a_seed_is_read_back_with_its_kind() {
        let seed = encode_seed(PREFIX_USER, &[7; 32]);
        assert!(seed.starts_with("SU"), "{seed}");
        let key = KeyPair::from_seed(&seed).unwrap();
        assert!(key.public_key().starts_with('U'));
        assert_eq!(key.public_key(), KeyPair::from_raw(PREFIX_USER, [7; 32]).public_key());
        let account = encode_seed(PREFIX_ACCOUNT, &[1; 32]);
        assert!(account.starts_with("SA"), "{account}");
        assert!(KeyPair::from_seed(&account).unwrap().public_key().starts_with('A'));
        // One character changed breaks the checksum.
        let mut bad = seed.clone().into_bytes();
        bad[10] = if bad[10] == b'A' { b'B' } else { b'A' };
        assert!(KeyPair::from_seed(std::str::from_utf8(&bad).unwrap()).is_err());
        assert!(KeyPair::from_seed(&key.public_key()).is_err(), "a public key isn't a seed");
    }

    #[test]
    fn a_nonce_is_signed_url_safe() {
        let key = KeyPair::from_raw(PREFIX_USER, [3; 32]);
        let sig = key.sign_nonce("PXoWU7zWAMt75FY");
        assert!(!sig.contains(['+', '/', '=']));
        let raw: [u8; 64] = b64::decode(&sig).unwrap().try_into().unwrap();
        assert!(key.verify(b"PXoWU7zWAMt75FY", &raw));
    }

    #[test]
    fn a_creds_file_is_read() {
        let seed = encode_seed(PREFIX_USER, &[9; 32]);
        let claims = b64::encode_url(br#"{"name":"alice","sub":"UABC"}"#);
        let jwt = format!("eyJ0eXAiOiJKV1QifQ.{claims}.c2ln");
        let text = format!("-----BEGIN NATS USER JWT-----\n{jwt}\n------END NATS USER JWT------\n\n************************* IMPORTANT *************************\nNKEY Seed printed below can be used to sign and prove identity.\n\n-----BEGIN USER NKEY SEED-----\n{seed}\n------END USER NKEY SEED------\n");
        let creds = parse_creds(&text).unwrap();
        assert_eq!(creds.jwt, jwt);
        assert_eq!(creds.key.public_key(), KeyPair::from_seed(&seed).unwrap().public_key());
        assert_eq!(jwt_claims(&creds.jwt).unwrap()["name"], "alice");
        assert!(parse_creds("nothing here").is_err());
        assert!(parse_creds(&format!("-----BEGIN X-----\n{jwt}\n-----END X-----\n")).is_err());
    }
}
