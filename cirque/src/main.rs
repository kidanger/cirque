use std::{path::PathBuf, str::FromStr};

use anyhow::Context;
use tokio::select;

use cirque_core::ServerState;
use cirque_server::{ConnectionLimiter, run_server};
use cirque_server::{TCPListener, TLSListener};
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};

mod config;

fn launch_server(
    config_path: PathBuf,
    server_state: ServerState,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    let config = config::Config::load_from_path(&config_path)
        .with_context(|| format!("loading config file {config_path:?}"))?;

    server_state.set_server_name(&config.server_name);
    let password = config.password.as_ref().map(|p| p.as_bytes());
    server_state.set_password(password);
    let motd = config
        .motd
        .as_ref()
        .map(|m| m.lines().map(|l| l.as_bytes().to_vec()).collect());
    server_state.set_motd(motd);
    server_state.set_default_channel_mode(&config.default_channel_mode);
    server_state.set_timeout_config(config.timeout_config());

    log::info!("config loaded");

    let connection_limiter = ConnectionLimiter::default();
    let future = if let Some(tls_config) = config.tls_config {
        let certs = CertificateDer::pem_file_iter(&tls_config.cert_file_path)
            .context(format!(
                "reading certificate file  {:?}",
                &tls_config.cert_file_path
            ))?
            .collect::<Result<Vec<_>, _>>()
            .context(format!(
                "reading certificate file  {:?}",
                &tls_config.cert_file_path
            ))?;

        let private_key =
            PrivateKeyDer::from_pem_file(&tls_config.private_key_file_path).context(format!(
                "reading private key file {:?}",
                &tls_config.private_key_file_path
            ))?;

        let listener = TLSListener::try_new(&config.address, config.port, certs, private_key)?;
        tokio::task::spawn(
            async move { run_server(listener, server_state, connection_limiter).await },
        )
    } else {
        let listener = TCPListener::try_new(&config.address, config.port)?;
        tokio::task::spawn(
            async move { run_server(listener, server_state, connection_limiter).await },
        )
    };

    Ok(future)
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    pretty_env_logger::formatted_builder()
        .filter_level(log::LevelFilter::Info)
        .try_init()?;

    let mut reload_signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;

    let Some(config_path) = std::env::args().nth(1) else {
        anyhow::bail!("missing <config_path> parameter. Usage: cirque <config_path>");
    };
    let config_path = PathBuf::from_str(&config_path)?;

    let server_state = {
        let config = config::Config::load_from_path(&config_path)?;
        let motd = config
            .motd
            .as_ref()
            .map(|motd| vec![motd.as_bytes().to_vec()]);
        let password = config.password.as_ref().map(|p| p.as_bytes().to_vec());
        ServerState::new(
            "cirque-server",
            &cirque_core::WelcomeConfig::default(),
            motd,
            password,
            config.timeout_config(),
        )
    };

    let mut server_handle = launch_server(config_path.clone(), server_state.clone())?;

    loop {
        select! {
            _ = reload_signal.recv() => {
                server_handle.abort();
            },
            result = &mut server_handle => {
                match result {
                    Ok(_) => {
                        unreachable!();
                    },
                    Err(err) =>{
                        match err.is_panic() {
                            true => {
                                log::error!("panic from the listener");
                                std::panic::resume_unwind(err.into_panic());
                            },
                            false => {
                                // otherwise, it's just an error due to cancellation of the task
                                // (when reloading the config)
                            },
                        }
                    },
                }

                match launch_server(config_path.clone(), server_state.clone()) {
                    Ok(s) => {
                        server_handle = s;
                    },
                    Err(err) => {
                        log::error!("error when relaunching the server: {err:#}");
                        log::error!("fix the config and send SIGHUP again (otherwise new clients cannot connect)");
                        server_handle = tokio::spawn(std::future::pending());
                    },
                };
            },
        }
    }
}
