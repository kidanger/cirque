use std::{fs, path::PathBuf, str::FromStr};

use yaml_rust2::{Yaml, YamlLoader};

pub struct Config {
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

    pub fn load_from_path(in_path: &PathBuf) -> Result<Self, anyhow::Error> {
        let string = fs::read_to_string(in_path)?;
        Config::load_from_str(string.as_str())
    }

    fn load_from_yaml(yaml: Vec<Yaml>) -> Result<Self, anyhow::Error> {
        let docs = yaml;
        let doc = docs
            .first()
            .ok_or(anyhow::anyhow!("invalid yaml document"))?;

        // Index access for map & array
        let tls_cert = yaml_path!(doc, "tls", "cert").as_str();
        let tls_key = yaml_path!(doc, "tls", "key").as_str();

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

#[cfg(test)]
mod tests {

    use std::fs;
    use std::{path::PathBuf, str::FromStr};

    use crate::config::Config;
    use rstest::*;
    use yaml_rust2::{Yaml, YamlLoader};

    fn default_yaml_path() -> Result<PathBuf, anyhow::Error> {
        let workspace_path = env!("CARGO_MANIFEST_DIR");
        Ok(PathBuf::from_str(workspace_path)?.join("../assets/default.yml"))
    }

    #[rstest]
    fn load_valid_config_from_path() -> Result<(), anyhow::Error> {
        let config = Config::load_from_path(&default_yaml_path()?)?;
        assert!(config.cert_file_path.is_some());
        assert!(config.private_key_file_path.is_some());

        Ok(())
    }

    #[rstest]
    fn load_valid_config_from_str() -> Result<(), anyhow::Error> {
        let config = Config::load_from_str(fs::read_to_string(default_yaml_path()?)?.as_str())?;
        assert!(config.cert_file_path.is_some());
        assert!(config.private_key_file_path.is_some());

        Ok(())
    }

    #[rstest]
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

    #[rstest]
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
