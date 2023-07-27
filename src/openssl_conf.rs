use std::{
    env,
    fs::{self},
    path::PathBuf,
};

use anyhow::{bail, Context};
use directories::BaseDirs;

const CNF_BYTES: &[u8] = include_bytes!("../openssl.cnf");

pub struct OpenSSLConf {
    path: PathBuf,
}

impl OpenSSLConf {
    pub fn new() -> anyhow::Result<Self> {
        let Some(cache_dir) = BaseDirs::new().map(|dirs| dirs.cache_dir().to_path_buf()) else {
            bail!("Failed to get cache dir");
        };
        let cnf_dir = cache_dir.join("netaccess");
        fs::create_dir_all(&cnf_dir).context("Failed to create openssl config directory")?;
        let path = cnf_dir.join("openssl.cnf");
        fs::write(&path, CNF_BYTES).context("Failed to write openssl config contents to file")?;
        env::set_var("OPENSSL_CONF", path.display().to_string());
        Ok(Self { path })
    }
}

impl Drop for OpenSSLConf {
    fn drop(&mut self) {
        if fs::remove_file(&self.path).is_err() {
            eprintln!(
                "Failed to remove openssl conf file {}, you may remove it manually",
                self.path.display()
            );
        }
    }
}
