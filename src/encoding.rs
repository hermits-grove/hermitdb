extern crate data_encoding;

pub fn decode(encoded: &String) -> Result<Vec<u8>, String> {
    data_encoding::BASE64URL.decode(encoded.as_bytes())
        .map_err(|e| format!("Failed decode {:?}", e))
}

pub fn encode(data: &Vec<u8>) -> String {
    data_encoding::BASE64URL.encode(data)
}
