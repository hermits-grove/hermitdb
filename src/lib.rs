#[macro_use]
extern crate serde_derive;
extern crate toml;
extern crate ring;

pub mod crypto;
pub mod encoding;
pub mod git_creds;
pub mod git_db;
pub mod manifest;
pub mod secret_meta;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
