extern crate ring;

use std;
use std::io::{Read, Write};

use self::ring::{aead, digest, pbkdf2};
use self::ring::rand::{SecureRandom, SystemRandom};

use error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct MasterKey([u8; 256 / 8]);

pub struct KDF {
    pub pbkdf2_iters: u32,
    pub salt: [u8; 256 / 8],
    pub entropy: [u8; 256 / 8]
}

impl KDF {
    pub fn master_key(&self, pass: &[u8]) -> MasterKey {
        let mut salt: Vec<u8> = Vec::with_capacity(512 / 8);
        salt.extend_from_slice(&self.entropy);
        salt.extend_from_slice(&self.salt);

        let mut master_key = MasterKey([0u8; 256 / 8]);
        pbkdf2::derive(
            &digest::SHA256,
            self.pbkdf2_iters,
            &salt,
            &pass,
            &mut master_key.0
        );
        master_key
    }
}

#[derive(Debug)]
pub struct Session {
    pub actor: u128,
    pub master_key: MasterKey
}

#[derive(Debug, PartialEq)]
pub struct Plaintext(pub Vec<u8>);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Encrypted{
    pub nonce: [u8; 96/8],
    pub ciphertext: Vec<u8>,
}

impl Plaintext {
    pub fn encrypt(&mut self, sess: &Session) -> Result<Encrypted> {
        // TAI: compress before encrypt
        let aead_algo = &aead::CHACHA20_POLY1305;
        let seal_key = aead::SealingKey::new(aead_algo, &sess.master_key.0)
            .map_err(|_| Error::Crypto("Failed to generate a sealing key".into()))?;


        let mut cryptic = Encrypted {
            nonce: rand_96()?,
            ciphertext: Vec::with_capacity(self.0.len() + aead_algo.tag_len())
        };
        
        cryptic.ciphertext.extend(&self.0);
        cryptic.ciphertext.extend(vec![0u8; aead_algo.tag_len()]);

        aead::seal_in_place(
            &seal_key,               // key
            &cryptic.nonce,          // nonce
            &cryptic.nonce,          // ad
            &mut cryptic.ciphertext, // plaintext
            seal_key.algorithm().tag_len()
        ).map_err(|_| Error::Crypto("Failed to encrypt with AEAD".into()))?;

        Ok(cryptic)
    }
}

impl Encrypted {
    pub fn decrypt(&self, sess: &Session) -> Result<Plaintext> {
        let aead_algo = &aead::CHACHA20_POLY1305;
        let opening_key = aead::OpeningKey::new(aead_algo, &sess.master_key.0)
            .map_err(|_| Error::Crypto("Failed to create key when decrypting".into()))?;

        let mut in_out = Vec::with_capacity(self.ciphertext.len());
        in_out.extend_from_slice(&self.ciphertext);

        let plain = aead::open_in_place(
            &opening_key,
            &self.nonce,
            &self.nonce,
            0,
            &mut in_out
        ).map_err(|_| Error::Crypto("Failed to decrypt".into()))?;

        Ok(Plaintext(plain.to_vec()))
    }
}

/// Will return Err if entropy_file does not exist
pub fn read_entropy_file(root: &std::path::Path) -> Result<[u8; 256/8]> {
    let entropy_filepath = root.join("entropy_file");
    let mut f = std::fs::File::open(&entropy_filepath)?;
    if std::fs::metadata(&entropy_filepath)?.len() != 256/8 {
        return Err(Error::State("entropy_file must contain exactly 256 bits".into()));
    }
    let mut entropy_file = [0u8; 256/8];
    f.read_exact(&mut entropy_file)?;
    Ok(entropy_file)
}

/// Will return Err if entropy_file exists
pub fn create_entropy_file(root: &std::path::Path) -> Result<[u8; 256/8]> {
    let entropy_filepath = root.join("entropy_file");
    if entropy_filepath.is_file() {
        return Err(Error::State("Attempting to create an entropy_file when one exists".into()));
    }

    let mut f = std::fs::File::create(entropy_filepath)?;
    let entropy = rand_256()?;
    f.write_all(&entropy)?;
    Ok(entropy)
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
    extern crate tempfile;
    use super::*;

    #[test]
    fn entropy_file() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_owned();
        let read1 = read_entropy_file(&dir_path);
        assert!(read1.is_err());

        let create1 = create_entropy_file(&dir_path);
        assert!(create1.is_ok());
        let entropy1 = create1.unwrap();
        
        let create2 = create_entropy_file(&dir_path);
        assert!(create2.is_err());

        assert_eq!(entropy1.len(), 256/8);

        let read2 = read_entropy_file(&dir_path);
        assert!(read2.is_ok());

        let entropy2 = read2.unwrap();
        assert_eq!(entropy1, entropy2);
    }

    #[test]
    fn kdf() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_owned();
        let kdf = KDF {
            pbkdf2_iters: 1000,
            salt: rand_256().unwrap(),
            entropy: create_entropy_file(&dir_path).unwrap()
        };

        let master_key1 = kdf.master_key("sssshh.. it's a secret".as_bytes());
        let master_key2 = kdf.master_key("sssshh.. it's a secret".as_bytes());
        let master_key3 = kdf.master_key("imposter!!".as_bytes());
        
        assert_eq!(master_key1, master_key2);
        assert_ne!(master_key1, master_key3);
    }

    #[test]
    fn plaintext_encrypt_decrypt() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_owned();
        
        let kdf = KDF {
            pbkdf2_iters: 1000,
            salt: rand_256().unwrap(),
            entropy: create_entropy_file(&dir_path).unwrap()
        };

        let sess = Session {
            actor: 0,
            master_key: kdf.master_key("do you KNOW who I am??".as_bytes())
        };

        let mut plain = Plaintext("I kinda like you".as_bytes().to_vec());
        let encrypted = plain.encrypt(&sess).unwrap();
        assert_ne!(encrypted.ciphertext, plain.0);
        
        let encrypted2 = plain.encrypt(&sess).unwrap();
        assert_ne!(encrypted.nonce, encrypted2.nonce);
        assert_ne!(encrypted.ciphertext, encrypted2.ciphertext);

        let plain2 = encrypted.decrypt(&sess).unwrap();
        let decrypted_string = String::from_utf8(plain2.0).unwrap();
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
