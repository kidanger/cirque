use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use std::{path::PathBuf, str::FromStr};

use tokio::select;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};

use cirque_core::ServerState;
use cirque_server::run_server;
use cirque_server::{AnyListener, TCPListener, TLSListener};

mod config;
use config::Config;

#[derive(Debug)]
struct FixedMOTDProvider(Option<String>);

impl cirque_core::MOTDProvider for FixedMOTDProvider {
    fn motd(&self) -> Option<Vec<Vec<u8>>> {
        self.0.as_ref().map(|motd| vec![motd.as_bytes().to_vec()])
    }
}

fn read_config(
    path: &Path,
) -> anyhow::Result<(Config, Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let config = config::Config::load_from_path(path)?;

    let certs = if let Some(path) = &config.cert_file_path {
        let mut file = File::open(path)?;
        rustls_pemfile::certs(&mut BufReader::new(&mut file)).collect::<Result<Vec<_>, _>>()?
    } else {
        anyhow::bail!("cannot load certificates");
    };

    let private_key = if let Some(path) = &config.private_key_file_path {
        let mut file = File::open(path)?;
        rustls_pemfile::private_key(&mut BufReader::new(&mut file))?
            .ok_or_else(|| anyhow::anyhow!("cannot load private key"))?
    } else {
        anyhow::bail!("cannot load private key");
    };

    Ok((config, certs, private_key))
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    pretty_env_logger::init();

    let welcome_config = cirque_core::WelcomeConfig::default();

    let mut reload_signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;

    let config_path = PathBuf::from_str("ircd.yml")?;
    let (server, server_state) = if let Ok((config, certs, private_key)) = read_config(&config_path)
    {
        let server_state = ServerState::new(
            &config.server_name,
            &welcome_config,
            Arc::new(FixedMOTDProvider(config.motd)),
            config.password.map(|p| p.as_bytes().into()),
        );
        let listener = TLSListener::try_new(certs, private_key).await?;

        (
            run_server(AnyListener::Tls(listener), server_state.clone()),
            server_state,
        )
    } else {
        let server_state = ServerState::new(
            "srv",
            &welcome_config,
            Arc::new(FixedMOTDProvider(None)),
            None,
        );
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
                let (config, _, _) = match read_config(&config_path) {
                    Ok(c) => c,
                    Err(err) => {
                        log::error!("cannot reload config: {err}");
                        continue;
                    }
                };

                server_state.set_server_name(&config.server_name);
                let password = config.password.as_ref().map(|p| p.as_bytes());
                server_state.set_password(password);
                let motd_provider = Arc::new(FixedMOTDProvider(config.motd));
                server_state.set_motd_provider(motd_provider);
                // TODO: tls reload
                log::info!("config reloaded");
            }
        }
    }

    Ok(())
}
