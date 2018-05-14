extern crate ring;
use std;

use db_error::{DBErr};
use encoding;

#[derive(Debug, PartialEq, Clone, Hash, Serialize, Deserialize)]
pub struct PathComp(String);

impl Eq for PathComp {}

#[derive(Debug, PartialEq)]
pub struct Path {
    components: Vec<PathComp>,
}

impl PathComp {
    pub fn escape(s: &str) -> PathComp {
        let mut escaped = String::with_capacity(s.len());
        for c in s.chars() {
            if Path::is_special_char(c) {
                escaped.push('\\');
            }
            escaped.push(c);
        }
        PathComp(escaped)
    }

    pub fn value(&self) -> &str {
        &self.0[..]
    }
}

impl Path {
    /// Construct the root path: "/"
    pub fn root() -> Path {
        Path {
            components: Vec::new()
        }
    }

    /// Construct a Path from a Path string
    ///
    /// Path strings formats rules:
    /// 0. Path string is composed of 0 or more components
    /// 1. A path with 0 components is called the `root` path and it is denoted by `/`
    /// 2. components are interspersed with '/'
    /// 3. special characters are '/' and '\'
    /// 3. a component may use any non-special unicode characters
    /// 4. to use a special characters in a component, escape by prefixing with `\`
    ///
    /// Examples:
    /// The "root" path:           `/`
    /// A three level path:        `/apple/black/x`
    /// Escaped `/` within a path: `/a\/b`
    pub fn new(s: &str) -> Result<Path, DBErr> {
        if s.len() == 0 || !s.starts_with("/") {
            return Err(DBErr::Parse(format!("Invalid Path \"{}\": Paths must begin with a '/'", s)));
        }
        
        if s.len() == 1 {
            // Root path case: "/"
            // we've already checked that the string starts with a `/`
            return Ok(Path {
                components: Vec::new()
            });
        }

        // TAI: is there a way to do this with no copy?
        let mut components: Vec<PathComp> = Vec::new();
        let mut escaping = false;
        let mut comp_start = 1;
        let mut pos = 1;
        for c in s[1..].chars() {
            if escaping {
                // this state is entered when previously read char was the escape char
                if !Path::is_special_char(c) {
                    return Err(DBErr::Parse(format!("Invalid path \"{}\": attempted to escape non-special character '{}'", s, c)));
                }
                escaping = false;
            } else if c == '\\' {
                // the escape char, enter escaping state
                escaping = true;
            } else if c == '/'{
                if pos - comp_start == 0 {
                    return Err(DBErr::Parse(format!("Invalid path \"{}\": attempted to create a path with an empty component", s)));
                }
                components.push(PathComp(s[comp_start..pos].into()));
                comp_start = pos + 1; // +1 skips the `/` seperator
            }
            pos += 1;
        }

        assert_eq!(pos, s.len());

        if escaping {
            return Err(DBErr::Parse(format!("Invalid path \"{}\": missing escaped special char after '\'", s)));
        }

        if pos - comp_start == 0 {
            // path looks like /a/b/c/ (the root path case has been checked above)
            return Err(DBErr::Parse(format!("Invalid path \"{}\": path must not end in '/'", s)));
        }

        components.push(PathComp(s[comp_start..pos].into()));

        Ok(Path {
            components: components
        })
    }

    pub fn is_root(&self) -> bool {
        self.components.len() == 0
    }

    pub fn parent(&self) -> Option<Path> {
        if self.is_root() {
            None
        } else {
            let path = Path {
                components: self.components[..(self.components.len() - 1)].to_vec()
            };
            Some(path)
        }
    }

    pub fn base_comp(&self) -> Option<&PathComp> {
        if self.is_root() {
            None
        } else {
            Some(&self.components[self.components.len() - 1])
        }
    }

    pub fn derive_filepath(&self, path_salt: &[u8]) -> std::path::PathBuf {
        let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
        ctx.update(&path_salt);
        // TAI: consider avoiding building the path string here
        //      we should be able to update the ctx with path components
        ctx.update(&self.to_string().into_bytes());
        let digest = ctx.finish();
        let encoded_hash = encoding::encode(&digest.as_ref());
        let (dir_part, file_part) = encoded_hash.split_at(2);
        let filepath = std::path::PathBuf::from(dir_part)
            .join(file_part);

        filepath
    }

    fn is_special_char(c: char) -> bool {
        c == '/' || c == '\\'
    }
}

impl ToString for Path {
    fn to_string(&self) -> String {
        if self.components.len() == 0 {
            // handle root path case
            return String::from("/");
        }

        // components are stored escaped
        let predicted_cap = self.components
            .iter()
            .map(|c| 1 + c.0.len())
            .sum();
        let mut path = String::with_capacity(predicted_cap);
        for comp in self.components.iter() {
            path.push('/');
            path.push_str(&comp.0);
        }
        assert_eq!(path.capacity(), predicted_cap); // we allocated just just enough
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root() {
        let r1 = Path::root();
        let r2 = Path::new("/").unwrap();
        assert_eq!(r1, r2);
        assert_eq!(r1.to_string(), "/");
    }

    #[test]
    fn new() {
        let root = Path::new("/").unwrap();
        assert_eq!(root.components.len(), 0);
        
        let p1 = Path::new("/a").unwrap();
        assert_eq!(p1.components, &[PathComp::escape("a")]);

        let p2 = Path::new(r"/a\\b/c\/").unwrap();
        assert_eq!(p2.components, &[
            PathComp::escape(r"a\b"), PathComp::escape(r"c/")
        ]);

        let bad_paths = &["", "//a", "/a/", r"/a\b", r"/\"];
        for p in bad_paths.iter() {
            let res = Path::new(&p);
            assert!(res.is_err());
        }
    }

    #[test]
    fn to_and_from_str() {
        let test_paths = ["/", "/a", r"/\/", r"/\\", "/ ", "/a/b/c", r"/a\\b/c\\"];
        for path in test_paths.iter() {
            assert_eq!(Path::new(&path).unwrap().to_string(), *path);
        }
    }

    #[test]
    fn derive_filepath() {
        let path_salt = "$";
        let filepath = Path::new("/a/b/c")
            .unwrap()
            .derive_filepath(&path_salt.as_bytes());

        //test vector comes from the python code:
        //>>> import hashlib
        //>>> hashlib.sha256(b"$/a/b/c").hexdigest()
        //'63b2c7879bd2a4d08a4671047a19fdd4c88e580efb66d853045a210eea0afe79'
        let expected = std::path::PathBuf::from("63")
            .join("b2c7879bd2a4d08a4671047a19fdd4c88e580efb66d853045a210eea0afe79");
        
        assert_eq!(filepath, expected);
    }

    #[test]
    fn path_comp_escape() {
        let test_vectors = [
            ("/", r"\/"),
            (r"\", r"\\"),
            (r"//\\", r"\/\/\\\\"),
            (r"a/b/c", r"a\/b\/c"),
            (r"a _^78!Ms-", r"a _^78!Ms-")
        ];

        for &(raw, escaped) in test_vectors.iter() {
            assert_eq!(PathComp::escape(raw).0, escaped);
        }
    }
}
