use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use std::{path::PathBuf, str::FromStr};

mod config;

use cirque::run_server;
use cirque::{AnyListener, ServerState};
use cirque::{TCPListener, TLSListener};
use config::Config;
use tokio::select;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};

#[derive(Debug)]
struct FixedMOTDProvider(Option<String>);

impl cirque::MOTDProvider for FixedMOTDProvider {
    fn motd(&self) -> Option<Vec<Vec<u8>>> {
        self.0.as_ref().map(|motd| vec![motd.as_bytes().to_vec()])
    }
}

fn read_config(
    path: &Path,
) -> anyhow::Result<(Config, Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let Ok(config) = config::Config::load_from_path(path) else {
        anyhow::bail!("cannot load config");
    };

    let mut certs = None;
    if let Some(ref cert_file_path) = config.cert_file_path {
        certs = Some(
            rustls_pemfile::certs(&mut BufReader::new(&mut File::open(cert_file_path)?))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    let mut private_key = None;
    if let Some(ref private_key_file_path) = config.private_key_file_path {
        private_key = rustls_pemfile::private_key(&mut BufReader::new(&mut File::open(
            private_key_file_path,
        )?))?;
    }

    if certs.is_some() && private_key.is_some() {
        Ok((config, certs.unwrap(), private_key.unwrap()))
    } else {
        anyhow::bail!("Config incomplete");
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let welcome_config = cirque::WelcomeConfig::default();

    let mut reload_signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;

    let config_path = PathBuf::from_str("ircd.yml")?;
    let (server, server_state) = if let Ok((config, certs, private_key)) = read_config(&config_path)
    {
        let server_state = ServerState::new(
            &config.server_name,
            &welcome_config,
            Arc::new(FixedMOTDProvider(config.motd)),
            config.password.map(|p| p.as_bytes().into()),
        )
        .shared();
        let listener = TLSListener::try_new(certs, private_key).await?;

        (
            run_server(AnyListener::Tls(listener), server_state.clone()),
            server_state,
        )
    } else {
        dbg!("listening without TLS on 6667");
        let server_state = ServerState::new(
            "srv",
            &welcome_config,
            Arc::new(FixedMOTDProvider(None)),
            None,
        )
        .shared();
        let listener = TCPListener::try_new(6667).await?;

        (
            run_server(AnyListener::Tcp(listener), server_state.clone()),
            server_state,
        )
    };

    let mut server = tokio::task::spawn(server);

    loop {
        select! {
            result = &mut server => {
                result??;
                break;
            },
            _ = reload_signal.recv() => {
                dbg!("hup");

                let Ok((config, _, _)) = read_config(&config_path) else {
                    dbg!("cannot read config?");
                    continue;
                };

                let mut server_state = server_state.lock().unwrap();
                let password = config.password.as_ref().map(|p| p.as_bytes());
                server_state.set_password(password);
                server_state.set_server_name(&config.server_name);
                let motd_provider = Arc::new(FixedMOTDProvider(config.motd));
                server_state.set_motd_provider(motd_provider);
            }
        }
    }

    Ok(())
}
