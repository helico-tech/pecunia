//! Password encryption of book files (ADR 0007): Argon2id turns the password into a key, XChaCha20-Poly1305 seals the
//! plain book document. The envelope is JSON, so a person can see what the file is.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{Result, StoreError};

pub const ENCRYPTED_FORMAT: &str = "pecunia-encrypted";
/// The newest envelope version this app reads.
const ENVELOPE_VERSION: u32 = 1;
const CIPHER: &str = "xchacha20poly1305";

/// A source of random bytes: the operating system, or the browser's `crypto.getRandomValues`.
pub type Random = fn(&mut [u8]) -> std::result::Result<(), String>;

/// How hard the key derivation works. New keys use [`KdfParams::DEFAULT`]; files may name others within limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl KdfParams {
    /// OWASP's Argon2id setting: 19 MiB, 2 passes, 1 lane.
    pub const DEFAULT: KdfParams = KdfParams { m_kib: 19 * 1024, t: 2, p: 1 };

    /// A file can't make us spend more than this (a hostile file could otherwise ask for gigabytes).
    fn check(self) -> Result<Self> {
        let ok = (8..=256 * 1024).contains(&self.m_kib) && (1..=10).contains(&self.t) && (1..=4).contains(&self.p) && self.m_kib >= 8 * self.p;
        if ok { Ok(self) } else { Err(StoreError::Format("the file's encryption settings are out of range".into())) }
    }
}

/// A key made from a password, with the salt and settings it was made with. Wiped from memory when dropped.
pub struct Key {
    bytes: Zeroizing<[u8; 32]>,
    salt: [u8; 16],
    params: KdfParams,
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(…)")
    }
}

impl Key {
    /// Derives the key for `password` with `salt`.
    pub fn derive(password: &str, salt: [u8; 16], params: KdfParams) -> Result<Key> {
        let params = params.check()?;
        let p = Params::new(params.m_kib, params.t, params.p, Some(32)).map_err(|e| StoreError::Format(format!("key settings: {e}")))?;
        let mut bytes = Zeroizing::new([0u8; 32]);
        Argon2::new(Algorithm::Argon2id, Version::V0x13, p)
            .hash_password_into(password.as_bytes(), &salt, bytes.as_mut())
            .map_err(|e| StoreError::Io(format!("can't make the key: {e}")))?;
        Ok(Key { bytes, salt, params })
    }

    /// A new key for `password` with a fresh random salt.
    pub fn new(password: &str, random: Random) -> Result<Key> {
        if password.is_empty() {
            return Err(StoreError::Io("the password is empty".into()));
        }
        let mut salt = [0u8; 16];
        random(&mut salt).map_err(StoreError::Io)?;
        Key::derive(password, salt, KdfParams::DEFAULT)
    }
}

#[derive(Serialize, Deserialize)]
struct Kdf {
    name: String,
    m_kib: u32,
    t: u32,
    p: u32,
    salt: String,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    format: String,
    version: u32,
    kdf: Kdf,
    cipher: String,
    nonce: String,
    data: String,
}

/// The header fields, bound to the ciphertext so nobody can swap them.
fn associated(version: u32, kdf: &Kdf, cipher: &str) -> Vec<u8> {
    format!("{ENCRYPTED_FORMAT}/{version}/{}/{}/{}/{}/{}/{cipher}", kdf.name, kdf.m_kib, kdf.t, kdf.p, kdf.salt).into_bytes()
}

/// Whether `bytes` is an encrypted book (without checking that it is a good one).
pub fn is_encrypted(bytes: &[u8]) -> bool {
    #[derive(Deserialize)]
    struct Head {
        format: String,
    }
    serde_json::from_slice::<Head>(bytes).is_ok_and(|h| h.format == ENCRYPTED_FORMAT)
}

/// Encrypts the plain book document `plain` with `key`; a fresh nonce for every call.
pub fn seal(plain: &[u8], key: &Key, random: Random) -> Result<Vec<u8>> {
    let mut nonce = [0u8; 24];
    random(&mut nonce).map_err(StoreError::Io)?;
    let kdf = Kdf { name: "argon2id".into(), m_kib: key.params.m_kib, t: key.params.t, p: key.params.p, salt: B64.encode(key.salt) };
    let aad = associated(ENVELOPE_VERSION, &kdf, CIPHER);
    let cipher = XChaCha20Poly1305::new(key.bytes.as_ref().into());
    let data =
        cipher.encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad: &aad }).map_err(|_| StoreError::Io("can't encrypt the book".into()))?;
    let env = Envelope {
        format: ENCRYPTED_FORMAT.into(),
        version: ENVELOPE_VERSION,
        kdf,
        cipher: CIPHER.into(),
        nonce: B64.encode(nonce),
        data: B64.encode(data),
    };
    let mut out = serde_json::to_vec_pretty(&env).map_err(|e| StoreError::Io(e.to_string()))?;
    out.push(b'\n');
    Ok(out)
}

/// How to unlock an envelope: a password, or a key kept from before (it only fits the same salt and settings).
pub enum Unlock<'a> {
    Password(&'a str),
    Key(&'a Key),
}

fn wrong() -> StoreError {
    StoreError::WrongPassword
}

/// Decrypts an envelope. Returns the plain book document and the key (to seal the next save with).
pub fn open(bytes: &[u8], unlock: Unlock, name: &str) -> Result<(Zeroizing<Vec<u8>>, Key)> {
    let env: Envelope = serde_json::from_slice(bytes).map_err(|e| StoreError::Format(format!("{name}: {e}")))?;
    if env.format != ENCRYPTED_FORMAT {
        return Err(StoreError::Format(format!("{name} is not an encrypted Pecunia book")));
    }
    if env.version > ENVELOPE_VERSION {
        return Err(StoreError::Format(format!("{name} was encrypted by a newer Pecunia (envelope version {})", env.version)));
    }
    if env.kdf.name != "argon2id" || env.cipher != CIPHER {
        return Err(StoreError::Format(format!("{name} uses an encryption this app doesn't know")));
    }
    let salt: [u8; 16] = B64.decode(&env.kdf.salt).ok().and_then(|s| s.try_into().ok()).ok_or_else(wrong)?;
    let nonce: [u8; 24] = B64.decode(&env.nonce).ok().and_then(|s| s.try_into().ok()).ok_or_else(wrong)?;
    let data = B64.decode(&env.data).map_err(|_| wrong())?;
    let params = KdfParams { m_kib: env.kdf.m_kib, t: env.kdf.t, p: env.kdf.p };
    let key = match unlock {
        Unlock::Password(pw) => Key::derive(pw, salt, params)?,
        Unlock::Key(k) if k.salt == salt && k.params == params => Key { bytes: k.bytes.clone(), salt, params },
        Unlock::Key(_) => return Err(StoreError::Locked),
    };
    let aad = associated(env.version, &env.kdf, &env.cipher);
    let cipher = XChaCha20Poly1305::new(key.bytes.as_ref().into());
    let plain = cipher.decrypt(XNonce::from_slice(&nonce), Payload { msg: &data, aad: &aad }).map_err(|_| wrong())?;
    Ok((Zeroizing::new(plain), key))
}

/// Random bytes from the operating system.
#[cfg(not(target_arch = "wasm32"))]
pub fn os_random(buf: &mut [u8]) -> std::result::Result<(), String> {
    getrandom::fill(buf).map_err(|e| format!("no random numbers: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cheap settings and a counter instead of randomness, so tests are fast and repeatable.
    fn fake_random(buf: &mut [u8]) -> std::result::Result<(), String> {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = i as u8;
        }
        Ok(())
    }

    fn key(pw: &str) -> Key {
        Key::derive(pw, [7; 16], KdfParams { m_kib: 64, t: 1, p: 1 }).unwrap()
    }

    #[test]
    fn seals_and_opens() {
        let k = key("hunter2");
        let sealed = seal(b"{\"format\":\"pecunia\"}", &k, fake_random).unwrap();
        assert!(is_encrypted(&sealed));
        assert!(!String::from_utf8_lossy(&sealed).contains("pecunia\\\""), "the book is not in the clear");
        let (plain, again) = open(&sealed, Unlock::Password("hunter2"), "x").unwrap();
        assert_eq!(&plain[..], b"{\"format\":\"pecunia\"}");
        assert!(open(&sealed, Unlock::Key(&again), "x").is_ok(), "the kept key opens the next save");
        assert_eq!(open(&sealed, Unlock::Password("wrong"), "x").unwrap_err(), StoreError::WrongPassword);
        let other = Key::derive("hunter2", [8; 16], KdfParams { m_kib: 64, t: 1, p: 1 }).unwrap();
        assert_eq!(open(&sealed, Unlock::Key(&other), "x").unwrap_err(), StoreError::Locked, "another salt needs the password");
    }

    #[test]
    fn tampering_is_caught() {
        let k = key("pw");
        let sealed = seal(b"secret", &k, fake_random).unwrap();
        let mut env: serde_json::Value = serde_json::from_slice(&sealed).unwrap();
        env["kdf"]["t"] = serde_json::json!(2);
        let swapped = serde_json::to_vec(&env).unwrap();
        assert!(open(&swapped, Unlock::Password("pw"), "x").is_err(), "a changed header fails");
        let mut env: serde_json::Value = serde_json::from_slice(&sealed).unwrap();
        let mut data = B64.decode(env["data"].as_str().unwrap()).unwrap();
        data[0] ^= 1;
        env["data"] = serde_json::json!(B64.encode(data));
        assert_eq!(open(&serde_json::to_vec(&env).unwrap(), Unlock::Password("pw"), "x").unwrap_err(), StoreError::WrongPassword);
    }

    #[test]
    fn hostile_envelopes_are_errors() {
        let cases: &[&[u8]] = &[
            b"",
            b"{}",
            br#"{"format":"pecunia-encrypted","version":1,"kdf":{"name":"argon2id","m_kib":4000000000,"t":1,"p":1,"salt":"AAAAAAAAAAAAAAAAAAAAAA=="},"cipher":"xchacha20poly1305","nonce":"","data":""}"#,
            br#"{"format":"pecunia-encrypted","version":9,"kdf":{"name":"argon2id","m_kib":64,"t":1,"p":1,"salt":""},"cipher":"x","nonce":"","data":""}"#,
            br#"{"format":"pecunia-encrypted","version":1,"kdf":{"name":"scrypt","m_kib":64,"t":1,"p":1,"salt":""},"cipher":"xchacha20poly1305","nonce":"","data":""}"#,
            br#"{"format":"pecunia-encrypted","version":1,"kdf":{"name":"argon2id","m_kib":64,"t":1,"p":1,"salt":"!!"},"cipher":"xchacha20poly1305","nonce":"!!","data":"!!"}"#,
        ];
        for (i, c) in cases.iter().enumerate() {
            assert!(open(c, Unlock::Password("pw"), "x").is_err(), "case {i}");
        }
        assert!(Key::new("", fake_random).is_err());
    }
}
