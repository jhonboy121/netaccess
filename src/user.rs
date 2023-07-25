use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone)]
pub struct User {
    name: String,
    password: String,
}

impl User {
    pub fn new(name: String, password: String) -> Self {
        Self { name, password }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn password(&self) -> &str {
        &self.password
    }
}

impl Display for User {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("User {}", self.name))
    }
}
