use keyring::{error::Result, keyutils, CredentialBuilder, Error};
use std::fmt::{self, Display, Formatter};

const SERVICE_ID: &str = "netaccess-usermanager";

#[derive(Clone, PartialEq, Eq)]
pub struct User {
    name: String,
    password: String,
}

impl Display for User {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("User {}", self.name))
    }
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

#[derive(Debug)]
pub struct UserManager {
    builder: Box<CredentialBuilder>,
}

impl Default for UserManager {
    fn default() -> Self {
        Self {
            builder: keyutils::default_credential_builder(),
        }
    }
}

impl UserManager {
    pub fn add_user(&self, user: &User) -> Result<()> {
        let credential = self.builder.build(None, SERVICE_ID, user.name())?;
        credential.set_password(&user.password)?;
        Ok(())
    }

    pub fn fetch_user(&self, user_name: &str) -> Result<User> {
        let credential = self.builder.build(None, SERVICE_ID, user_name)?;
        credential.get_password().map(|password| User {
            name: user_name.to_owned(),
            password,
        })
    }

    pub fn update_user(&self, user: &User) -> Result<()> {
        let credential = self.builder.build(None, SERVICE_ID, user.name())?;
        credential.set_password(&user.password)?;
        Ok(())
    }

    pub fn delete_user(&self, user_name: &str) -> Result<()> {
        let credential = self.builder.build(None, SERVICE_ID, user_name)?;
        match credential.delete_password() {
            Ok(_) | Err(Error::NoEntry) => Ok(()),
            other => other,
        }
    }
}
