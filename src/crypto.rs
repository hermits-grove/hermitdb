use std;
use std::io::{stdin, Read, Write, stdout};

use ring::{aead, digest, pbkdf2};
use ring::rand::{SecureRandom, SystemRandom};

use db_error::{DBErr};

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    version: u8,
    pbkdf2_iters: u32,
    pbkdf2_salt: [u8; 256/8],
    consumed: bool
}

#[derive(Debug)]
pub struct Session {
    root: std::path::PathBuf,
    key_file: Option<[u8; 256/8]>,
    password: Option<Vec<u8>>
}

#[derive(Debug, PartialEq)]
pub struct Plaintext {
    pub data: Vec<u8>,
    pub config: Config
}

#[derive(Debug, PartialEq)]
pub struct Block {
    pub ciphertext: Vec<u8>,
    pub config: Config
}

impl Session {
    pub fn new(root: &std::path::Path) -> Session {
        Session {
            root: root.to_path_buf(),
            key_file: None,
            password: None
        }
    }

    pub fn create_key_file(&mut self) -> Result<(), DBErr> {
        let key_file_path = self.root.join("key_file");
        if key_file_path.is_file() {
            return Err(DBErr::State(String::from("Attempting to create a key_file when one exists")));
        }

        assert!(self.key_file.is_none());

        let key_file = gen_rand_256()?;
        let mut f = std::fs::File::create(key_file_path).map_err(DBErr::IO)?;
        f.write_all(&key_file).map_err(DBErr::IO)?;

        self.key_file = Some(key_file);
        Ok(())
    }

    pub fn set_pass(&mut self, pass: &[u8]) {
        self.password = Some(pass.to_vec());
    }

    pub fn key_file(&mut self) -> Result<[u8; 256/8], DBErr> {
        if let Some(ref key_file) = self.key_file {
            Ok(key_file.clone())
        } else {
            let key_file_path = self.root.join("key_file");
            if !key_file_path.is_file() {
                return Err(DBErr::State(String::from("Attempting to read key_file when one does not exist")));
            }
            
            let mut f = std::fs::File::open(&key_file_path).map_err(DBErr::IO)?;

            assert_eq!(std::fs::metadata(&key_file_path).map_err(DBErr::IO)?.len(), 256/8);

            let mut key_file = [0u8; 256/8];
            f.read_exact(&mut key_file).map_err(DBErr::IO)?;
            self.key_file = Some(key_file.clone());
            Ok(key_file)
        }
    }

    pub fn pass(&mut self) -> Result<Vec<u8>, DBErr> {
        if let Some(ref pass) = self.password {
            Ok(pass.clone())
        } else {
            let pass = read_stdin("master passphrase")?;
            let pass_bytes = pass.as_bytes().to_vec();
            self.password = Some(pass_bytes.clone());
            Ok(pass_bytes)
        }
    }
}

impl Config {
    pub fn fresh_default() -> Result<Config, DBErr> {
        let salt = gen_rand_256()?;

        Ok(Config {
            version: 0,
            pbkdf2_iters: 100000,
            pbkdf2_salt: salt,
            consumed: false
        })
    }

    pub fn serialized_byte_count() -> usize {
        1 + 32 / 8 + 256 / 8 // version + pbkdf2 iters + pbkdf2 salt
    }

    /// Parses a config from some bytes
    ///
    /// Reads just enough of the prefix of the given bytes to parse a `Config`
    /// does not look past what's required to parse a valid `Config`
    ///
    /// format: [ version (1 byte) | iters (4 bytes) | salt (256 / 8 = 32 bytes) ]
    pub fn from_bytes(bytes: &[u8]) -> Result<Config, DBErr> {
        if bytes.len() == 0 {
            return Err(DBErr::Parse(String::from("Nothing to parse (bytes.len() == 0)")));
        }

        let version = bytes[0];

        match version {
            0 => {
                let expected_bytes = Config::serialized_byte_count();
                if bytes.len() < expected_bytes {
                    Err(DBErr::Parse(format!("Not enough bytes to parse config: {} < {}", bytes.len(), expected_bytes)))
                } else {
                    let iters = bytes_to_u32(&[bytes[1], bytes[2], bytes[3], bytes[4]]);
                    let mut salt = [0u8; 256/8];
                    salt.copy_from_slice(&bytes[(1+4)..(1+4+(256/8))]);
                    Ok(Config {
                        version: 0,
                        pbkdf2_iters: iters,
                        pbkdf2_salt: salt,
                        consumed: true
                    })
                }
            },
            _ => Err(DBErr::Version(format!("Unknown Config version: {}", version)))
        }
    }

    /// Serializes config to bytes
    /// - only config data needed for decryption is serialized
    ///
    /// format: [ version (1 byte) | iters (4 bytes) | salt (256 / 8 = 32 bytes) ]
    pub fn to_bytes(&self) -> Vec<u8> {
        let byte_count = Config::serialized_byte_count();
        let mut bytes: Vec<u8> = Vec::with_capacity(byte_count);
        bytes.push(self.version);
        bytes.extend(u32_to_bytes(self.pbkdf2_iters).iter());
        bytes.extend(self.pbkdf2_salt.iter());
        
        assert_eq!(bytes.capacity(), byte_count); // allocation sanity check

        bytes
    }
}

impl Plaintext {
    pub fn encrypt(&mut self, sess: &mut Session) -> Result<Block, DBErr> {
        if self.config.consumed {
            return Err(DBErr::Crypto(String::from("Attempted to encrypt with an already consumed config")));
        }

        assert_eq!(self.config.consumed, false);
        let ciphertext = encrypt(&sess.pass()?, &sess.key_file()?, &self.data, &mut self.config)?;
        assert_eq!(self.config.consumed, true);
        
        let block = Block {
            ciphertext: ciphertext.to_vec(),
            config: self.config.clone()
        };
        Ok(block)
    }
}

impl Block {
    pub fn read(file_path: &std::path::Path) -> Result<Block, DBErr> {
        let mut f = std::fs::File::open(file_path)
            .map_err(DBErr::IO)?;
        
        let mut config_bytes = vec![0u8; Config::serialized_byte_count()];
        f.read_exact(&mut config_bytes)
            .map_err(DBErr::IO)?;
        let config = Config::from_bytes(&config_bytes)?;

        let mut data = Vec::new();
        f.read_to_end(&mut data)
            .map_err(DBErr::IO)?;

        Ok(Block {
            ciphertext: data,
            config: config
        })
    }
    
    pub fn write(&self, file_path: &std::path::Path) -> Result<(), DBErr> {
        let mut f = std::fs::File::create(file_path)
            .map_err(DBErr::IO)?;

        f.write_all(&self.config.to_bytes())
            .or_else(|e| {
                std::fs::remove_file(file_path)
                    .map_err(DBErr::IO)?;
                Err(DBErr::IO(e))
            })?;
        
        f.write_all(&self.ciphertext)
            .or_else(|e| {
                std::fs::remove_file(file_path)
                    .map_err(DBErr::IO)?;
                Err(DBErr::IO(e))
            })
    }

    pub fn decrypt(&self, sess: &mut Session) -> Result<Plaintext, DBErr> {
        let plaintext_data = decrypt(&sess.pass()?, &sess.key_file()?, &self.ciphertext, &self.config)?;
        let plaintext = Plaintext {
            data: plaintext_data.to_vec(),
            config: self.config.clone()
        };
        Ok(plaintext)
    }
}

/// Generate a 256 bit key derived through PBKDF2_SHA256
pub fn pbkdf2(pass: &[u8], config: &Config) -> Result<[u8; 256 / 8], DBErr> {
    if config.version != 0 {
        return Err(DBErr::Version(format!("Config version {} not supported", config.version)));
    }

    let pbkdf2_algo = &digest::SHA256;
    let mut key = [0u8; 256 / 8];
    pbkdf2::derive(pbkdf2_algo, config.pbkdf2_iters, &config.pbkdf2_salt, &pass, &mut key);
    Ok(key)
}

pub fn encrypt(pass: &[u8], key_file: &[u8; 256/8], data: &[u8], config: &mut Config) -> Result<Vec<u8>, DBErr> {
    // TAI: short ciphertexts may give away useful length information to an attacker, consider padding plaintext before encrypting

    if config.version != 0 {
        return Err(DBErr::Version(format!("Config version {} not supported", config.version)));
    }

    if config.consumed {
        return Err(DBErr::Crypto(String::from("ATTEMPTING TO ENCRYPT WITH A SALT WHICH HAS ALREADY BEEN CONSUMED")));
    }

    config.consumed = true;

    let aead_algo = &aead::CHACHA20_POLY1305;
    
    let mut key: [u8; 256/8] = pbkdf2(pass, &config)?;
    for i in 0..key.len() {
        key[i] = key[i] ^ key_file[i];
    }

    let seal_key = aead::SealingKey::new(aead_algo, &key)
        .map_err(|_| DBErr::Crypto(String::from("Failed to generate a sealing key")))?;

    let mut in_out = Vec::with_capacity(data.len() + seal_key.algorithm().tag_len());
    in_out.extend(data.iter());
    in_out.extend(vec![0u8; seal_key.algorithm().tag_len()]);
    let ad = config.to_bytes();
    let nonce = [0u8; 96 / 8]; // see crypto design doc (we never encrypt with the same key twice)

    aead::seal_in_place(&seal_key, &nonce, &ad, &mut in_out, seal_key.algorithm().tag_len())
        .map_err(|_| DBErr::Crypto(String::from("Failed to encrypt with AEAD")))?;

    Ok(in_out)
}

pub fn decrypt(pass: &[u8], key_file: &[u8; 256/8], ciphertext: &[u8], config: &Config) -> Result<Vec<u8>, DBErr> {
    assert!(config.consumed);
    let aead_algo = &aead::CHACHA20_POLY1305;
    let mut key: [u8; 256/8] = pbkdf2(pass, &config)?;
    for i in 0..key.len() {
        key[i] = key[i] ^ key_file[i];
    }
    
    let opening_key = aead::OpeningKey::new(aead_algo, &key)
        .map_err(|_| DBErr::Crypto(String::from("Failed to create key when decrypting")))?;

    let mut in_out = Vec::new();
    in_out.extend(ciphertext.iter());

    let ad = config.to_bytes();
    let nonce = [0u8; 96 / 8]; // see crypto design doc (we never encrypt with the same key twice)

    let plaintext = aead::open_in_place(&opening_key, &nonce, &ad, 0, &mut in_out)
        .map_err(|_| DBErr::Crypto(String::from("Failed to decrypt")))?;

    Ok(plaintext.to_vec())
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

pub fn read_stdin(prompt: &str) -> Result<String, DBErr> {
    // TODO: for unix systems, do something like this: https://stackoverflow.com/a/37416107

    print!("{}: ", prompt);
    stdout().flush().ok();
    let mut pass = String::with_capacity(16);
    stdin().read_line(&mut pass)
        .map_err(DBErr::IO)?;
    pass.trim();
    Ok(pass)
}

pub fn gen_rand(bytes: usize) -> Result<Vec<u8>, DBErr> {
    let mut buf = vec![0u8; bytes];
    // TAI: Should this rng live in a session so we don't have to recreate it each time?
    SystemRandom::new()
        .fill(&mut buf)
        .map_err(|_| DBErr::Crypto(String::from("Failed to generate random pbkdf2 salt")))?;
    Ok(buf)
}

// It's quite common to need 256 bits of crypto grade random.
//
// Having a fixed size array here gives us some compile time guarantees.
pub fn gen_rand_256() -> Result<[u8; 256/8], DBErr> {
    let mut buf = [0u8; 256/8];
    // TAI: Should this rng live in a session so we don't have to recreate it each time?
    SystemRandom::new()
        .fill(&mut buf)
        .map_err(|_| DBErr::Crypto(String::from("Failed to generate 256 bits of random")))?;
    Ok(buf)
}

#[cfg(test)]
mod test {
    extern crate tempfile;
    use super::*;
    
    #[test]
    fn fresh_config_is_not_consumed() {
        let conf = Config::fresh_default().unwrap();
        assert_eq!(conf.consumed, false);
    }

    #[test]
    fn config_is_same_but_consumed_after_converted_to_bytes() {
        let conf1 = Config::fresh_default().unwrap();
        let conf2 = Config::from_bytes(&conf1.to_bytes()).unwrap();
        assert_eq!(conf1.consumed, false);
        assert_eq!(conf2.consumed, true);
        assert_eq!(conf1.version, conf2.version);
        assert_eq!(conf1.pbkdf2_iters, conf2.pbkdf2_iters);
        assert_eq!(conf1.pbkdf2_salt, conf2.pbkdf2_salt);
    }

    #[test]
    fn config_fails_to_deserialize_bad_version() {
        let mut conf = Config::fresh_default().unwrap();
        conf.version = 183;
        match Config::from_bytes(&conf.to_bytes()).unwrap_err() {
            DBErr::Version(_) => ":)",
            _ => panic!("should have err'd with a version error")
        };
    }

    #[test]
    fn config_fails_to_deserialize_bad_bytes() {
        match Config::from_bytes(&[0]).unwrap_err() {
            DBErr::Parse(_) => ":)",
            _ => panic!("should have err'd with a parse error")
        };
    }

    #[test]
    fn session() {
        let dir = tempfile::tempdir().unwrap();
        let mut sess = Session::new(&dir.path());

        assert_eq!(sess.root, dir.path());
        assert!(sess.password.is_none());
        assert!(sess.key_file.is_none());

        sess.create_key_file().unwrap();

        let key_file_path = dir.path().join("key_file");
        assert!(key_file_path.is_file());
        assert_eq!(std::fs::metadata(&key_file_path).unwrap().len(), 256/8);

        let mut key_file_data = [0u8; 256/8];
        let mut f = std::fs::File::open(&key_file_path).unwrap();
        f.read_exact(&mut key_file_data).unwrap();

        assert_eq!(sess.key_file().unwrap(), key_file_data);

        let mut new_sess = Session::new(&dir.path());
        assert_eq!(sess.key_file().unwrap(), new_sess.key_file().unwrap());

        assert!(sess.password.is_none());
        sess.set_pass("this is between you and me".as_bytes());
        assert_eq!(std::str::from_utf8(&sess.pass().unwrap()).unwrap(), "this is between you and me");
    }

    #[test]
    fn plaintext_encrypt_decrypt() {
        let msg = "I love you";
        let mut plain = Plaintext {
            data: msg.as_bytes().to_vec(),
            config: Config::fresh_default().unwrap()
        };
        let mut sess = Session {
            root: tempfile::tempdir().unwrap().path().to_path_buf(),
            key_file: Some(gen_rand_256().unwrap()),
            password: Some(gen_rand(12).unwrap())
        };

        let encrypted = plain.encrypt(&mut sess).unwrap();
        assert_eq!(plain.config.consumed, true);
        assert_eq!(plain.config.version, encrypted.config.version);
        assert_eq!(plain.config.pbkdf2_iters, encrypted.config.pbkdf2_iters);
        assert_eq!(plain.config.pbkdf2_salt, encrypted.config.pbkdf2_salt);
        
        match plain.encrypt(&mut sess).unwrap_err() {
            DBErr::Crypto(_) => ":)",
            _ => panic!("Encrypting with a consumed config should fail!")
        };

        let plain2 = encrypted.decrypt(&mut sess).unwrap();
        assert_eq!(plain2.config.consumed, true);
        assert_eq!(plain.config.version, plain2.config.version);
        assert_eq!(plain.config.pbkdf2_iters, plain2.config.pbkdf2_iters);
        assert_eq!(plain.config.pbkdf2_salt, plain2.config.pbkdf2_salt);

        let decrypted_string = String::from_utf8(plain2.data).unwrap();
        assert_eq!(decrypted_string, "I love you");
    }

    #[test]
    fn encrypt_decrypt_file_io() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("secret_note.txt");
        let msg = "she's becoming a peaceful one";

        let mut plain = Plaintext {
            data: msg.as_bytes().to_vec(),
            config: Config::fresh_default().unwrap()
        };

        let mut sess = Session {
            root: tempfile::tempdir().unwrap().path().to_path_buf(),
            key_file: Some(gen_rand_256().unwrap()),
            password: Some(gen_rand(12).unwrap())
        };

        let encrypted = plain.encrypt(&mut sess)
            .unwrap();
        
        encrypted.write(&file_path).unwrap();

        let encrypted2 = Block::read(&file_path)
            .unwrap();
        assert_eq!(encrypted, encrypted2);
        
        let plain2 = encrypted2
            .decrypt(&mut sess)
            .unwrap();
        
        assert_eq!(plain, plain2);
        
        let decrypted_string = String::from_utf8(plain2.data).unwrap();
        assert_eq!(decrypted_string, "she's becoming a peaceful one");
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
