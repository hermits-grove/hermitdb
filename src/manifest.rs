use toml;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Manifest {
    pub version: String,
    pub entries: Vec<Entry>
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Entry {
    
    // e.g ["pass", "social", "www.facebook.com"] represents the file "pass/social/www.facebook.com"
    pub path: String,
    
     // e.g. ["password", "social"] - used to improve queries 
    pub tags: Vec<String>,
    
    // e.g. "lj01g7OD8g30F6X9" - files are stored in top level directory with garbled names - done to avoid leaking information from structure of repository
    // this entry represents the files ./<garbled> and ./<garbled>.toml
    pub garbled_path: String
}


#[derive(Debug, Clone)]
pub struct EntryRequest {
    
    // e.g ["pass", "social", "www.facebook.com"] represents the file "pass/social/www.facebook.com"
    pub path: String,
    
     // e.g. ["password", "social"] - used to improve queries 
    pub tags: Vec<String>
}

impl Manifest {
    pub fn empty() -> Manifest {
        Manifest {
            version: String::from("0.0.1"),
            entries: Vec::new()
        }
    }
    pub fn from_toml_bytes(bytes: &Vec<u8>) -> Result<Manifest, String> {
        toml::from_slice(&bytes)
            .map_err(|e| format!("Failed to read Manifest from TOML: {:?}", e))
    }

    pub fn to_toml_bytes(&self) -> Result<Vec<u8>, String> {
        toml::to_vec(&self)
            .map_err(|e| format!("Failed to serialize manifest {:?}", e))
    }
}

impl EntryRequest {
    pub fn new(path: &String, tags: &Vec<String>) -> EntryRequest{
        EntryRequest {
            path: path.clone(),
            tags: tags.clone()
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        for comp in self.path.split("/") {
            let trimmed = comp.trim();

            if comp == "/" || comp == "\\" {
                return Err(format!("lookup path must not start or end in '/' or '\\'"))
            }
            if comp.len() == 0 {
                return Err(format!("Found empty path component: '{}'\nlookup paths should not start or end with a component seperator", comp));
            }
            
            if trimmed.len() != comp.len() {
                return Err(format!("Lookup path component '{}' contains extra whitespace", comp));
            }
        }
        Ok(())
    }
}
