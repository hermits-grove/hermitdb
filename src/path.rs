use db_error::{DBErr};

#[derive(Debug, PartialEq)]
pub struct Path<'a> {
    components: Vec<&'a str>,
}

impl<'a> Path<'a> {
    /// Construct the root path: "/"
    pub fn root() -> Path<'a> {
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
    pub fn new(s: &'a str) -> Result<Path<'a>, DBErr> {
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

        // TAI: store components escaped instead of going through the unescaping/escaping ceremony
        // TAI: is there a way to do this with no copy?
        let mut components: Vec<&'a str> = Vec::new();
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
                components.push(&s[comp_start..pos]);
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

        components.push(&s[comp_start..pos]);

        Ok(Path {
            components: components
        })
    }

    fn is_special_char(c: char) -> bool {
        c == '/' || c == '\\'
    }
}

impl<'a> ToString for Path<'a> {
    fn to_string(&self) -> String {
        if self.components.len() == 0 {
            // handle root path case
            return String::from("/");
        }

        // components are stored escaped
        let predicted_cap = self.components.iter().map(|c| 1 + c.len()).sum();
        let mut path = String::with_capacity(predicted_cap);
        for comp in self.components.iter() {
            path.push('/');
            path.push_str(comp);
        }
        assert_eq!(path.capacity(), predicted_cap); // sanity check that our math was right
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
        assert_eq!(p1.components, &["a"]);

        let p2 = Path::new(r"/a\\b/c\/").unwrap();
        assert_eq!(p2.components, &[r"a\\b", r"c\/"]);

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
}









