use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context;
use serde::Deserialize;

use cirque_core::ChannelMode;

#[derive(Debug, Deserialize)]
pub struct TlsConfig {
    #[serde(rename = "cert")]
    pub cert_file_path: PathBuf,
    #[serde(rename = "key")]
    pub private_key_file_path: PathBuf,
}

#[serde_with::serde_as]
#[derive(Debug, Deserialize)]
struct TimeoutConfig {
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    pub base: Duration,
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    pub reduced: Duration,
}

impl From<&TimeoutConfig> for cirque_core::TimeoutConfig {
    fn from(val: &TimeoutConfig) -> Self {
        cirque_core::TimeoutConfig {
            base_timeout: val.base,
            reduced_timeout: val.reduced,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server_name: String,
    pub password: Option<String>,
    pub motd: Option<String>,
    pub port: u16,
    pub address: String,
    #[serde(rename = "tls")]
    pub tls_config: Option<TlsConfig>,
    #[serde(deserialize_with = "deserialize_channel_mode")]
    pub default_channel_mode: ChannelMode,
    timeout: Option<TimeoutConfig>,
}

fn deserialize_channel_mode<'de, D>(value: D) -> Result<ChannelMode, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;

    impl serde::de::Visitor<'_> for Visitor {
        type Value = ChannelMode;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a string corresponding to a channel mode")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            ChannelMode::try_from(v).map_err(E::custom)
        }
    }

    value.deserialize_str(Visitor)
}

impl Config {
    pub fn load_from_str(str: &str) -> Result<Self, anyhow::Error> {
        let config: Config = serde_yml::from_str(str)?;
        Ok(config)
    }

    pub fn load_from_path(path: &Path) -> Result<Self, anyhow::Error> {
        let string = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file {path:?}"))?;
        Config::load_from_str(string.as_str())
    }
}

impl Config {
    pub fn timeout_config(&self) -> Option<cirque_core::TimeoutConfig> {
        self.timeout
            .as_ref()
            .map(|tc| -> cirque_core::TimeoutConfig { tc.into() })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic_in_result_fn)]

    use std::{path::PathBuf, str::FromStr};

    use crate::config::Config;

    fn default_yaml_path() -> anyhow::Result<PathBuf> {
        let workspace_path = env!("CARGO_MANIFEST_DIR");
        Ok(PathBuf::from_str(workspace_path)?.join("../config.yml"))
    }

    #[test]
    fn load_valid_config_from_path() -> anyhow::Result<()> {
        let config = Config::load_from_path(&default_yaml_path()?)?;
        assert!(config.tls_config.is_some());

        Ok(())
    }
}
