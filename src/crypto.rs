use std::path::Path;
use std::fs::File;
use std::io::{stdin, Read, Write, stdout};

use ring::{aead, digest, pbkdf2};
use ring::rand::{SecureRandom, SystemRandom};

use secret_meta::Meta;
use encoding;

pub struct Session {
    password: Option<String>
}

pub struct Plaintext {
    pub data: Vec<u8>,
    pub meta: Meta
}

pub struct Encrypted {
    data: Vec<u8>,
    meta: Meta
}

impl Session {
    pub fn new() -> Session {
        Session {
            password: None
        }
    }

    pub fn pass(&mut self) -> Result<String, String> {
        if let Some(pass) = self.password.clone() {
            Ok(pass)
        } else {
            let pass = read_stdin("master passphrase", true)?;
            self.password = Some(pass.clone());
            Ok(pass)
        }
    }
}

impl Plaintext {
    pub fn encrypt(&self, sess: &mut Session) -> Result<Encrypted, String> {
        let pass = sess.pass()?;
        let encrypted = Encrypted {
            data: encrypt(&pass.as_bytes(), &self.data, &self.meta)?,
            meta: self.meta.clone()
        };
        Ok(encrypted)
    }
}

impl Encrypted {
    pub fn read(path: &Path) -> Result<Encrypted, String> {
        let mut f = File::open(path)
            .map_err(|e| format!("Failed to open {:?}: {:?}", path, e))?;

        let mut data = Vec::new();
        f.read_to_end(&mut data)
            .map_err(|e| format!("Failed read to {:?}: {:?}", path, e))?;

        let meta = Meta::from_toml(&path.with_extension("toml"))?;

        Ok(Encrypted {
            data: data,
            meta: meta
        })
    }
    
    pub fn write(&self, path: &Path) -> Result<(), String> {
        File::create(path)
            .map_err(|e| format!("Failed to create {:?}: {:?}", path, e))
            .and_then(|mut f| {
                // write encrypted data to disk
                f.write_all(&self.data)
                    .map_err(|e| format!("Failed write to {:?}: {:?}", path, e))
            })
            .and_then(|_| {
                // write encryption metadata to disk
                self.meta.write_toml(&path.with_extension("toml"))
            })
    }

    pub fn decrypt(&self, sess: &mut Session) -> Result<Plaintext, String> {
        let pass = sess.pass()?;
        let plaintext = Plaintext {
            data: decrypt(&pass.as_bytes(), &self.data, &self.meta)?,
            meta: self.meta.clone()
        };
        Ok(plaintext)
    }
}

pub fn pbkdf2(pass: &[u8], keylen: u32, meta: &Meta) -> Result<Vec<u8>, String> {
    if meta.pbkdf2.algo != "Sha256" {
        panic!("only 'Sha256' implemented for pbkdf2");
    }
    if keylen < 128 {
        panic!("key is too short! keylen (bits): {}", keylen);
    }
    if keylen % 8 != 0 {
        panic!("Key length should be a multiple of 8, got: {}", keylen);
    }

    let pbkdf2_algo = &digest::SHA256;
    let salt = encoding::decode(&meta.pbkdf2.salt)?;
    let mut key = vec![0u8; (keylen / 8) as usize];
    pbkdf2::derive(pbkdf2_algo, meta.pbkdf2.iters, &salt, &pass, &mut key);
    Ok(key)
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

pub fn encrypt(pass: &[u8], data: &[u8], meta: &Meta) -> Result<Vec<u8>, String> {
    let aead_algo = match meta.aead.algo.as_str() {
        "ChaCha20-Poly1305" => &aead::CHACHA20_POLY1305,
        _ => panic!("AEAD supports only 'ChaCha20-Poly1305'")
    };

    if meta.plaintext.min_bits % 8 != 0 {
        panic!("Plaintext min_bits must be a multiple of 8");
    }

    let key = pbkdf2(pass, meta.aead.keylen, &meta)?;
    let seal_key = aead::SealingKey::new(aead_algo, &key)
        .map_err(|e| format!("Failed to create sealing key: {:?}", e))?;

    let pad_bits = ((meta.plaintext.min_bits as i64) - (data.len() * 8) as i64).max(0) as u32;
    let pad_data = generate_rand_bits(pad_bits)?;
    let pad_bits_data = u32_to_bytes(pad_bits);
    let mut in_out = Vec::with_capacity(
        pad_bits_data.len()
            + pad_data.len()
            + data.len()
            + seal_key.algorithm().tag_len()
    );
    in_out.extend(pad_bits_data.iter());
    in_out.extend(pad_data.iter());
    in_out.extend(data.iter());
    in_out.extend(vec![0u8; seal_key.algorithm().tag_len()]);
    let ad: &[u8] = &meta.to_toml_bytes()?;
    let nonce = encoding::decode(&meta.aead.nonce)?;

    aead::seal_in_place(&seal_key, &nonce, &ad, &mut in_out, seal_key.algorithm().tag_len())
        .map_err(|e| format!("Failed to seal: {:?}", e))?;

    Ok(in_out)
}

pub fn decrypt(pass: &[u8], encrypted_data: &[u8], meta: &Meta) -> Result<Vec<u8>, String> {
    if meta.aead.algo != "ChaCha20-Poly1305" {
        panic!("only 'ChaCha20-Poly1305' implemented for aead");
    }

    let aead_algo = &aead::CHACHA20_POLY1305;

    let key = pbkdf2(pass, meta.aead.keylen, &meta)?;
    let opening_key = aead::OpeningKey::new(aead_algo, &key).unwrap();

    let mut in_out = Vec::new();
    in_out.extend(encrypted_data.iter());

    let ad: &[u8] = &meta.to_toml_bytes()?;
    let nonce = encoding::decode(&meta.aead.nonce)?;

    let plaintext = aead::open_in_place(&opening_key, &nonce, &ad, 0, &mut in_out)
        .map_err(|_| String::from("Failed to decrypt"))
        .map(|plaintext| plaintext.to_vec())?;

    if plaintext.len() < 4 {
        panic!("We expect at least 4 bytes of plaintext for pad bits length");
    }

    // first 4 bytes is a u32 with length of paddding
    let pad_bits = bytes_to_u32(
        &[plaintext[0], plaintext[1], plaintext[2], plaintext[3]]
    );
    
    if plaintext.len() < (4 + pad_bits / 8) as usize {
        panic!("We expect at least 4 + pad_bits / 8 bytes of data in plaintext");
    }
    Ok(plaintext[(4 + (pad_bits / 8) as usize)..].to_vec())
}

pub fn read_stdin(prompt: &str, obscure_input: bool) -> Result<String, String> {
    // TODO: for unix systems, do something like this: https://stackoverflow.com/a/37416107
    // TODO: obscure_input is ignored currently

    print!("{}: ", prompt);
    stdout().flush().ok();
    let mut pass = String::new();
    stdin().read_line(&mut pass)
        .map_err(|e| format!("Error reading password from stdin: {}", e))
        .map(|_| pass.trim().to_string())
}

pub fn generate_rand_bits(n: u32) -> Result<Vec<u8>, String> {
    if n % 8 != 0 {
        return Err(format!("Bits to generate must be a multiple of 8, got: {}", n));
    }

    let mut buff = vec![0u8; (n / 8) as usize ];
    let rng = SystemRandom::new();
    rng.fill(&mut buff)
        .map_err(|e| format!("Failed to generate random bits: {:?}", e))?;
    Ok(buff)
}
