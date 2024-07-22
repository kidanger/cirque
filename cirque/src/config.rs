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
        // Debug support
        println!("{:?}", doc);

        // Index access for map & array
        let tls_cert = doc["tls"]["cert"].as_str();
        let tls_key = doc["tls"]["cert"].as_str();

        let path_tls_cert = tls_cert
            .map(|s| PathBuf::from_str(s))
            .map_or(None, |r| Some(r.unwrap()));

        let private_key_file_path = tls_key
            .map(|s| PathBuf::from_str(s))
            .map_or(None, |r| Some(r.unwrap()));

        Ok(Self {
            cert_file_path: path_tls_cert,
            private_key_file_path: private_key_file_path,
        })
    }
}
