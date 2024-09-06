use std::fs::File;
use std::io::BufReader;
use std::{path::PathBuf, str::FromStr};

use tokio::select;

use cirque_core::ServerState;
use cirque_server::{run_server, ConnectionLimiter};
use cirque_server::{AnyListener, TCPListener, TLSListener};

mod config;

fn launch_server(
    config_path: PathBuf,
    server_state: ServerState,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    let config = config::Config::load_from_path(&config_path)?;

    server_state.set_server_name(&config.server_name);
    let password = config.password.as_ref().map(|p| p.as_bytes());
    server_state.set_password(password);
    server_state.set_motd(
        config
            .motd
            .as_ref()
            .map(|motd| vec![motd.as_bytes().to_vec()]),
    );
    server_state.set_default_channel_mode(&config.default_channel_mode.unwrap_or_default());

    log::info!("config reloaded");

    let connection_limiter = ConnectionLimiter::default();
    let future = if let Some(tls_config) = config.tls_config {
        let certs = {
            let mut file = File::open(tls_config.cert_file_path)?;
            rustls_pemfile::certs(&mut BufReader::new(&mut file)).collect::<Result<Vec<_>, _>>()?
        };

        let private_key = {
            let mut file = File::open(tls_config.private_key_file_path)?;
            rustls_pemfile::private_key(&mut BufReader::new(&mut file))?
                .ok_or_else(|| anyhow::anyhow!("cannot load private key"))?
        };

        let listener = TLSListener::try_new(&config.address, config.port, certs, private_key)?;
        tokio::task::spawn(async move {
            let listener = AnyListener::Tls(listener);
            run_server(listener, server_state, connection_limiter).await
        })
    } else {
        let listener = TCPListener::try_new(&config.address, config.port)?;
        tokio::task::spawn(async move {
            let listener = AnyListener::Tcp(listener);
            run_server(listener, server_state, connection_limiter).await
        })
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
        ServerState::new(
            "cirque-server",
            &cirque_core::WelcomeConfig::default(),
            config
                .motd
                .as_ref()
                .map(|motd| vec![motd.as_bytes().to_vec()]),
            config.password.map(|p| p.as_bytes().to_vec()),
        )
    };

    let mut server = launch_server(config_path.clone(), server_state.clone())?;

    loop {
        select! {
            _ = reload_signal.recv() => {
                server.abort();
            },
            result = &mut server => {
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
                        server = s;
                    },
                    Err(err) => {
                        log::error!("error when relaunching the server: {err}");
                        log::error!("fix the config and send SIGHUP again (otherwise new clients cannot connect)");
                        server = tokio::spawn(std::future::pending());
                    },
                };
                log::info!("recreated the listener");
            },
        }
    }
}
