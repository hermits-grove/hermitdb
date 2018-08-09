extern crate ring;

use std::fmt::{self, Debug};

use self::ring::{aead, hmac, hkdf, digest, pbkdf2};
use self::ring::rand::{SecureRandom, SystemRandom};

use error::{Error, Result};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KDF {
    pub pbkdf2_iters: u32,
    pub salt: [u8; 256 / 8]
}

pub struct KeyHierarchy {
    key: hmac::SigningKey
}

#[derive(Debug, PartialEq, Eq)]
pub struct CryptoKey {
    key: [u8; 256 / 8]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Encrypted{
    pub nonce: [u8; 96/8],
    pub ciphertext: Vec<u8>,
}

impl KDF {
    pub fn derive_root(&self, pass: &[u8]) -> KeyHierarchy {
        let mut root_key = [0u8; 256 / 8];

        pbkdf2::derive(
            &digest::SHA256,
            self.pbkdf2_iters,
            &self.salt,
            &pass,
            &mut root_key
        );

        let key = hmac::SigningKey::new(&digest::SHA256, &root_key);

        KeyHierarchy { key }
    }
}

impl KeyHierarchy {
    pub fn derive_child(&self, namespace: &[u8]) -> KeyHierarchy {
        KeyHierarchy {
            key: hkdf::extract(&self.key, namespace)
        }
    }

    pub fn signing_key(&self) -> &hmac::SigningKey {
        &self.key
    }

    pub fn key_for(&self, plaintext_unique_id: &[u8]) -> CryptoKey {
        let mut crypto_key = CryptoKey {
            key: [0u8; 256 / 8]
        };

        hkdf::expand(&self.key, plaintext_unique_id, &mut crypto_key.key);

        crypto_key
    }
}

impl CryptoKey {
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Encrypted> {
        let algo = &aead::CHACHA20_POLY1305;

        let mut cryptic = Encrypted {
            nonce: rand_96()?,
            ciphertext: vec![0u8; plaintext.len() + algo.tag_len()]
        };

        cryptic.ciphertext.splice(0..plaintext.len(), plaintext.iter().cloned());

        // TODO: sanity check, rm this once you've convinced yourself
        assert_eq!(&cryptic.ciphertext[0..plaintext.len()], plaintext);

        let key = aead::SealingKey::new(algo, &self.key)
            .map_err(|_| Error::Crypto("Failed to create a sealing key".into()))?;

        aead::seal_in_place(
            &key,                    // crypto key
            &cryptic.nonce,          // nonce
            &cryptic.nonce,          // ad
            &mut cryptic.ciphertext, // plaintext (encrypted in place)
            algo.tag_len()
        ).map_err(|_| Error::Crypto("Failed to encrypt".into()))?;

        Ok(cryptic)
    }

    pub fn decrypt(&self, encrypted: &Encrypted) -> Result<Vec<u8>> {
        let algo = &aead::CHACHA20_POLY1305;

        let key = aead::OpeningKey::new(algo, &self.key)
            .map_err(|_| Error::Crypto("Failed to create an opening key".into()))?;

        let mut ciphertext = encrypted.ciphertext.clone();
        let plain = aead::open_in_place(
            &key,                   // crypto key
            &encrypted.nonce,       // nonce
            &encrypted.nonce,       // ad
            0,                      // prefix padding (in bytes) to discard
            &mut ciphertext         // cyphertext (decrypted in place)
        ).map_err(|_| Error::Crypto("Failed to decrypt".into()))?;

        Ok(plain.to_vec())
    }
}

impl Debug for KeyHierarchy {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "KeyHierarchy")
    }
}

pub fn u32_to_bytes(i: u32) -> [u8; 4] {
    // output is in little endian
    // u32_to_bytes(0x12345678) -> [0x12, 0x34, 0x56, 0x78]
    return [
        ((i >> 24) & 0xff) as u8,
        ((i >> 16) & 0xff) as u8,
        ((i >> 8) & 0xff) as u8,
        (i & 0xff) as u8
    ]
}

pub fn bytes_to_u32(xs: &[u8; 4]) -> u32 {
    // `xs` is assumed to be little endian
    ((xs[0] as u32) << 24)
        | ((xs[1] as u32) << 16)
        | ((xs[2] as u32) << 8)
        | (xs[3] as u32)
}

pub fn rand_96() -> Result<[u8; 96/8]> {
    let mut buf = [0u8; 96/8];
    // TAI: Should this rng live in a session so we don't have to recreate it each time?
    SystemRandom::new()
        .fill(&mut buf)
        .map_err(|_| Error::Crypto("Failed to generate 96 bits of random".into()))?;
    Ok(buf)
}

pub fn rand_256() -> Result<[u8; 256/8]> {
    let mut buf = [0u8; 256/8];
    // TAI: Should this rng live in a session so we don't have to recreate it each time?
    SystemRandom::new()
        .fill(&mut buf)
        .map_err(|_| Error::Crypto("Failed to generate 256 bits of random".into()))?;
    Ok(buf)
}

#[cfg(test)]
mod test {
    use super::*;

    impl PartialEq for KeyHierarchy {
        fn eq(&self, other: &Self) -> bool {
            // we compare key's by signing the 0 byte... for testing it's probably alright
            hmac::sign(&self.key, &[0u8]).as_ref() == hmac::sign(&other.key, &[0u8]).as_ref()
        }
    }

    #[test]
    fn kdf() {
        let kdf = KDF {
            pbkdf2_iters: 1000,
            salt: rand_256().unwrap()
        };

        let root_key1 = kdf.derive_root("sssshh.. it's a secret".as_bytes());
        let root_key2 = kdf.derive_root("sssshh.. it's a secret".as_bytes());
        let imposter_key = kdf.derive_root("imposter!!".as_bytes());
        
        assert_eq!(root_key1, root_key2); // proof: kdf is deterministic
        assert_ne!(root_key1, imposter_key) // proof: varied password => varied key
    }

    #[test]
    fn key_hierarchy() {
        let kdf = KDF {
            pbkdf2_iters: 1000,
            salt: rand_256().unwrap()
        };

        let root_key = kdf
            .derive_root("pass".as_bytes());
        let log_key = root_key
            .derive_child("log".as_bytes());

        assert_ne!(root_key, log_key);

        let log_key2 = root_key
            .derive_child("log".as_bytes());

        assert_eq!(log_key, log_key2); // proof: derive_child is deterministic.

        let storage_key = root_key
            .derive_child("storage".as_bytes());

        assert_ne!(log_key, storage_key); // proof: varied child name => varied key

        let log_msg1_key = log_key
            .key_for(&[1u8]);

        let log_msg2_key = log_key
            .key_for(&[2u8]);

        let storage_block_key = storage_key
            .key_for(&[1u8]);

        assert_ne!(log_msg1_key, log_msg2_key);
        assert_ne!(storage_block_key, log_msg1_key);
        assert_ne!(storage_block_key, log_msg2_key);
    }

    #[test]
    fn plaintext_encrypt_decrypt() {
        let kdf = KDF {
            pbkdf2_iters: 1000,
            salt: rand_256().unwrap()
        };

        let root_key = kdf
            .derive_root("password".as_bytes());
        
        let msg = "I kinda like you".as_bytes();
        let msg_id = [0u8, 1u8];

        let key = root_key.key_for(&msg_id);

        let cryptic = key
            .encrypt(&msg)
            .unwrap();

        // can we do a better check than this?
        // maybe we can make some test vectors?
        assert_ne!(cryptic.ciphertext, msg);

        // Our encryption scheme must be probabilistic!
        //
        // The same msg encrypted under the same key should produce
        // different ciphertexts.
        let cryptic2 = key
            .encrypt(&msg)
            .unwrap();

        assert_ne!(cryptic.nonce, cryptic2.nonce);           // nonces must differ!
        assert_ne!(cryptic.ciphertext, cryptic2.ciphertext); // ciphertexts must differ!

        let decrypted_msg = key.decrypt(&cryptic).unwrap();
        let decrypted_string = String::from_utf8(decrypted_msg).unwrap();
        assert_eq!(decrypted_string, "I kinda like you");
    }

    #[test]
    fn u32_bytes_conversions() {
        assert_eq!(u32_to_bytes(65), [0, 0, 0, 0x41]);
        assert_eq!(u32_to_bytes(48023143), [0x02, 0xDC, 0xC6, 0x67]);
        assert_eq!(u32_to_bytes(0x12345678), [0x12, 0x34, 0x56, 0x78]);
        
        assert_eq!(bytes_to_u32(&[0, 0, 0, 0x41]), 65);
        assert_eq!(bytes_to_u32(&[0x02, 0xDC, 0xC6, 0x67]), 48023143);
        assert_eq!(bytes_to_u32(&[0x12, 0x34, 0x56, 0x78]), 0x12345678);

        assert_eq!(bytes_to_u32(&u32_to_bytes(35230)), 35230);
    }
}
