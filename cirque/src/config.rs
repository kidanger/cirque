use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use yaml_rust2::{Yaml, YamlLoader};

pub struct Config {
    pub server_name: String,
    pub password: Option<String>,
    pub motd: Option<String>,
    pub cert_file_path: Option<PathBuf>,
    pub private_key_file_path: Option<PathBuf>,
}

#[macro_export]
macro_rules! yaml_path {
    ($d:expr, $( $x:expr ),* ) => {
        {
            let mut temp =$d;
            $(
                temp = &temp[$x];
            )*
            temp
        }
    };
}

impl Config {
    pub fn load_from_str(in_str: &str) -> Result<Self, anyhow::Error> {
        let docs = YamlLoader::load_from_str(in_str)?;
        Config::load_from_yaml(docs)
    }

    pub fn load_from_path(in_path: &Path) -> Result<Self, anyhow::Error> {
        let string = fs::read_to_string(in_path)?;
        Config::load_from_str(string.as_str())
    }

    fn load_from_yaml(yaml: Vec<Yaml>) -> Result<Self, anyhow::Error> {
        let docs = yaml;
        let doc = docs
            .first()
            .ok_or(anyhow::anyhow!("invalid yaml document"))?;

        // Index access for map & array
        let Some(server_name) = yaml_path!(doc, "server_name").as_str().map(Into::into) else {
            anyhow::bail!("config: missing field `server_name`");
        };
        let password = yaml_path!(doc, "password").as_str().map(Into::into);
        let motd = yaml_path!(doc, "motd").as_str().map(Into::into);

        let tls_cert = yaml_path!(doc, "tls", "cert").as_str();
        let tls_key = yaml_path!(doc, "tls", "key").as_str();

        let cert_file_path = tls_cert.map(PathBuf::from_str).transpose()?;
        let private_key_file_path = tls_key.map(PathBuf::from_str).transpose()?;

        Ok(Self {
            server_name,
            password,
            motd,
            cert_file_path,
            private_key_file_path,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic_in_result_fn)]

    use std::fs;
    use std::{path::PathBuf, str::FromStr};

    use crate::config::Config;
    use yaml_rust2::{Yaml, YamlLoader};

    fn default_yaml_path() -> Result<PathBuf, anyhow::Error> {
        let workspace_path = env!("CARGO_MANIFEST_DIR");
        Ok(PathBuf::from_str(workspace_path)?.join("../assets/default.yml"))
    }

    #[test]
    fn load_valid_config_from_path() -> Result<(), anyhow::Error> {
        let config = Config::load_from_path(&default_yaml_path()?)?;
        assert!(config.cert_file_path.is_some());
        assert!(config.private_key_file_path.is_some());

        Ok(())
    }

    #[test]
    fn load_valid_config_from_str() -> Result<(), anyhow::Error> {
        let config = Config::load_from_str(fs::read_to_string(default_yaml_path()?)?.as_str())?;
        assert!(config.cert_file_path.is_some());
        assert!(config.private_key_file_path.is_some());

        Ok(())
    }

    #[test]
    fn load_valid_config() -> Result<(), anyhow::Error> {
        let mut docs =
            YamlLoader::load_from_str(fs::read_to_string(default_yaml_path()?)?.as_str())?;
        let mut doc = docs[0].clone();

        // Index access for map & array
        let certif_path = "assets/default.yml";
        let key_path = "assets/default.yml";
        doc["tls"]["cert"] = Yaml::from_str(certif_path);
        doc["tls"]["key"] = Yaml::from_str(key_path);

        docs[0] = doc;

        let config = Config::load_from_yaml(docs)?;
        assert!(config.cert_file_path.is_some());
        assert!(config.private_key_file_path.is_some());
        Ok(())
    }

    #[test]
    fn load_unvalid_config() -> Result<(), anyhow::Error> {
        let mut docs =
            YamlLoader::load_from_str(fs::read_to_string(default_yaml_path()?)?.as_str())?;
        let mut doc = docs[0].clone();

        // Index access for map & array
        let certif_path = "0";
        let key_path = "assets/default.yml";
        doc["tls"]["cert"] = Yaml::from_str(certif_path);
        doc["tls"]["key"] = Yaml::from_str(key_path);

        docs[0] = doc;

        let config = Config::load_from_yaml(docs)?;
        assert!(config.cert_file_path.is_none());
        assert!(config.private_key_file_path.is_some());
        Ok(())
    }
}
