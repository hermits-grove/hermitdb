extern crate data_encoding;
use error::{Result, Error};
    
pub fn decode(encoded: &str) -> Result<Vec<u8>> {
    data_encoding::HEXLOWER.decode(encoded.as_bytes())
        .map_err(Error::DataEncodingDecode)
}

pub fn encode(data: &[u8]) -> String {
    data_encoding::HEXLOWER.encode(data)
}
