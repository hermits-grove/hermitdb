extern crate data_encoding;

pub fn decode(encoded: &str) -> Result<Vec<u8>, String> {
    data_encoding::HEXLOWER.decode(encoded.as_bytes())
        .map_err(|e| format!("Failed decode {:?}", e))
}

pub fn encode(data: &[u8]) -> String {
    data_encoding::HEXLOWER.encode(data)
}
