use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;

use toml;

use git_db;
use crypto;
use encoding;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Meta {
    pub version: String,
    pub plaintext: Plaintext,
    pub pbkdf2: PBKDF2,
    pub aead: AEAD,
    pub paranoid: Paranoid
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Plaintext {
    pub min_bits: u32
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PBKDF2 {
    pub algo: String,
    pub iters: u32,
    pub salt: String
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AEAD {
    pub algo: String,
    pub nonce: String,
    pub keylen: u32
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Paranoid {
    pub simple_multiple_encryption: String,
    pub cascading_encryption: String
}

impl Meta {
    pub fn default_meta() -> Result<Meta, String> {
        Ok(Meta {
            version: String::from("0.0.1"),
            plaintext: Plaintext {
                min_bits: 1024
            },
            pbkdf2: PBKDF2 {
                algo: String::from("Sha256"),
                iters: 1_000_000,
                salt: encoding::encode(&crypto::generate_rand_bits(96)?)
            },
            aead: AEAD {
                algo: String::from("ChaCha20-Poly1305"),
                nonce: encoding::encode(&crypto::generate_rand_bits(96)?),
                keylen: 256
            },
            paranoid: Paranoid {
                simple_multiple_encryption: String::from("TBD"),
                cascading_encryption: String::from("TBD")
            }
        })
    }
    
    pub fn generate_secure_meta(db: &git_db::DB) -> Result<Meta, String> {
        let default_meta = Meta::default_meta()?;
        
        let salt = crypto::generate_rand_bits(96)?;
        let nonce = db.generate_nonce()?;
        
        let meta = Meta {
            pbkdf2: PBKDF2 {
                salt: encoding::encode(&salt),
                ..default_meta.pbkdf2.clone()
            },
            aead: AEAD {
                nonce: encoding::encode(&nonce),
                ..default_meta.aead.clone()
            },
            ..default_meta.clone()
        };
        Ok(meta)
    }

    pub fn from_toml(path: &PathBuf) -> Result<Meta, String> {
        File::open(path)
            .map_err(|e| format!("Failed to open {:?}: {:?}", path, e))
            .and_then(|mut f| {
                let mut contents = Vec::new();
                f.read_to_end(&mut contents)
                    .map_err(|e| format!("Failed to read {:?}: {:?}", path, e))
                    .map(|_| contents)
            })
            .and_then(|contents| {
                toml::from_slice(&contents).map_err(|e| format!("{:?}", e))
            })
    }

    pub fn to_toml_bytes(&self) -> Result<Vec<u8>, String> {
        toml::to_vec(&self)
            .map_err(|e| format!("Failed to serialize meta {:?}", e))
    }

    pub fn write_toml(&self, path: &PathBuf) -> Result<(), String> {
        self.to_toml_bytes()
            .and_then(|serialized_meta| {
                File::create(&path)
                    .map_err(|e| format!("Failed to create {:?}: {:?}", path, e))
                    .and_then(|mut f| {
                        f.write_all(&serialized_meta)
                            .map_err(|e| format!("Failed write to meta file {:?}: {:?}", path, e))
                    })
            })
    }
}
