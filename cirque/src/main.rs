use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use std::{path::PathBuf, str::FromStr};

mod config;

use cirque::run_server;
use cirque::ServerState;
use cirque::{TCPListener, TLSListener};

#[derive(Debug)]
struct FixedMOTDProvider(Option<String>);

impl cirque::MOTDProvider for FixedMOTDProvider {
    fn motd(&self) -> Option<Vec<Vec<u8>>> {
        match &self.0 {
            Some(motd) => Some(vec![motd.as_bytes().to_vec()]),
            None => None,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let welcome_config = cirque::WelcomeConfig::default();

    // on SIGHUP, reload the config (motd) and cert files (even if their paths didn't change)

    let config_path = PathBuf::from_str("ircd.yml")?;
    if let Ok(config) = config::Config::load_from_path(&config_path) {
        let mut certs = None;
        if let Some(cert_file_path) = config.cert_file_path {
            certs = Some(
                rustls_pemfile::certs(&mut BufReader::new(&mut File::open(cert_file_path)?))
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
        let mut private_key = None;
        if let Some(private_key_file_path) = config.private_key_file_path {
            private_key = rustls_pemfile::private_key(&mut BufReader::new(&mut File::open(
                private_key_file_path,
            )?))?;
        }

        if certs.is_some() && private_key.is_some() {
            let server_state = ServerState::new(
                &config.server_name,
                &welcome_config,
                Arc::new(FixedMOTDProvider(config.motd)),
                config.password.map(|p| p.as_bytes().into()),
            );
            let listener = TLSListener::try_new(certs.unwrap(), private_key.unwrap()).await?;
            run_server(listener, server_state).await
        } else {
            anyhow::bail!("Config incomplete");
        }
    } else {
        dbg!("listening without TLS on 6667");
        let server_state = ServerState::new(
            "srv",
            &welcome_config,
            Arc::new(FixedMOTDProvider(None)),
            None,
        );
        let listener = TCPListener::try_new(6667).await?;
        run_server(listener, server_state).await
    }
}
