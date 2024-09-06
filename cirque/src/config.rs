use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::Context;
use cirque_core::ChannelMode;
use yaml_rust2::{Yaml, YamlLoader};

pub struct TlsConfig {
    pub cert_file_path: PathBuf,
    pub private_key_file_path: PathBuf,
}

pub struct Config {
    pub server_name: String,
    pub password: Option<String>,
    pub motd: Option<String>,
    pub port: u16,
    pub address: String,
    pub tls_config: Option<TlsConfig>,
    pub default_channel_mode: Option<ChannelMode>,
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
    pub fn load_from_str(str: &str) -> Result<Self, anyhow::Error> {
        let docs = YamlLoader::load_from_str(str)?;
        Config::load_from_yaml(docs)
    }

    pub fn load_from_path(path: &Path) -> Result<Self, anyhow::Error> {
        let string = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file {path:?}"))?;
        Config::load_from_str(string.as_str())
    }

    fn load_from_yaml(yaml: Vec<Yaml>) -> Result<Self, anyhow::Error> {
        #![allow(clippy::indexing_slicing)]
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
        let default_channel_mode = yaml_path!(doc, "default_channel_mode")
            .as_str()
            .map(TryInto::try_into)
            .transpose()
            .map_err(|e: String| anyhow::anyhow!(e))?;
        let Some(Ok(port)) = yaml_path!(doc, "port").as_i64().map(TryInto::try_into) else {
            anyhow::bail!("config: missing field `port` (or cannot parse as u16");
        };
        let Some(address) = yaml_path!(doc, "address").as_str().map(Into::into) else {
            anyhow::bail!("config: missing field `address`");
        };

        let tls_config = if !yaml_path!(doc, "tls").is_badvalue() {
            let Some(tls_cert) = yaml_path!(doc, "tls", "cert").as_str() else {
                anyhow::bail!("`tls.certificate` field is not specified");
            };
            let Some(tls_key) = yaml_path!(doc, "tls", "key").as_str() else {
                anyhow::bail!("`tls.key` field is not specified");
            };

            let cert_file_path = PathBuf::from_str(tls_cert)?;
            let private_key_file_path = PathBuf::from_str(tls_key)?;

            Some(TlsConfig {
                cert_file_path,
                private_key_file_path,
            })
        } else {
            None
        };

        Ok(Self {
            server_name,
            password,
            motd,
            port,
            address,
            tls_config,
            default_channel_mode,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic_in_result_fn)]
    #![allow(clippy::indexing_slicing)]

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
        assert!(config.tls_config.is_some());

        Ok(())
    }

    #[test]
    fn load_valid_config_from_str() -> Result<(), anyhow::Error> {
        let config = Config::load_from_str(fs::read_to_string(default_yaml_path()?)?.as_str())?;
        assert!(config.tls_config.is_some());

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
        assert!(config.tls_config.is_some());
        Ok(())
    }

    #[test]
    fn load_config_without_tls() -> Result<(), anyhow::Error> {
        let mut docs =
            YamlLoader::load_from_str(fs::read_to_string(default_yaml_path()?)?.as_str())?;
        let mut doc = docs[0].clone();

        doc["tls"] = yaml_rust2::Yaml::BadValue;
        docs[0] = doc;

        let config = Config::load_from_yaml(docs)?;
        assert!(config.tls_config.is_none());
        Ok(())
    }
}
