extern crate data_encoding;
use db_error::{Result, DBErr};
    
pub fn decode(encoded: &str) -> Result<Vec<u8>> {
    data_encoding::HEXLOWER.decode(encoded.as_bytes())
        .map_err(DBErr::DataEncodingDecode)
}

pub fn encode(data: &[u8]) -> String {
    data_encoding::HEXLOWER.encode(data)
}
