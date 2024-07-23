use std::{fs, path::PathBuf, str::FromStr};

use yaml_rust2::YamlLoader;

pub struct Config {
    pub cert_file_path: Option<PathBuf>,
    pub private_key_file_path: Option<PathBuf>,
}

impl Config {
    pub fn new(in_path: &PathBuf) -> Result<Self, anyhow::Error> {
        let string = fs::read_to_string(in_path)?;
        let docs = YamlLoader::load_from_str(string.as_str())?;
        let doc = &docs[0];

        // Index access for map & array
        let tls_cert = doc["tls"]["cert"].as_str();
        let tls_key = doc["tls"]["key"].as_str();

        let mut cert_file_path: Option<PathBuf> = None;
        if let Some(tls_cert) = tls_cert {
            cert_file_path = Some(PathBuf::from_str(tls_cert)?);
        }

        let mut private_key_file_path: Option<PathBuf> = None;
        if let Some(tls_key) = tls_key {
            private_key_file_path = Some(PathBuf::from_str(tls_key)?);
        }

        Ok(Self {
            cert_file_path,
            private_key_file_path,
        })
    }
}
