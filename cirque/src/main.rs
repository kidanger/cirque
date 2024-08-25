use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use std::{path::PathBuf, str::FromStr};

use tokio::select;

use cirque_core::ServerState;
use cirque_server::run_server;
use cirque_server::{AnyListener, TCPListener, TLSListener};

mod config;

#[derive(Debug)]
struct FixedMOTDProvider(Option<String>);

impl cirque_core::MOTDProvider for FixedMOTDProvider {
    fn motd(&self) -> Option<Vec<Vec<u8>>> {
        self.0.as_ref().map(|motd| vec![motd.as_bytes().to_vec()])
    }
}

fn launch_server(
    config_path: PathBuf,
    server_state: ServerState,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    let config = config::Config::load_from_path(&config_path)?;

    server_state.set_server_name(&config.server_name);
    let password = config.password.as_ref().map(|p| p.as_bytes());
    server_state.set_password(password);
    let motd_provider = Arc::new(FixedMOTDProvider(config.motd));
    server_state.set_motd_provider(motd_provider);

    log::info!("config reloaded");

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
            run_server(listener, server_state).await
        })
    } else {
        let listener = TCPListener::try_new(&config.address, config.port)?;
        tokio::task::spawn(async move {
            let listener = AnyListener::Tcp(listener);
            run_server(listener, server_state).await
        })
    };

    Ok(future)
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    pretty_env_logger::init();

    let mut reload_signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;

    let Some(config_path) = std::env::args().nth(1) else {
        anyhow::bail!("missing <config_path> parameter. Usage: cirque <config_path>");
    };
    let config_path = PathBuf::from_str(&config_path)?;

    let server_state = ServerState::new(
        "cirque-server",
        &cirque_core::WelcomeConfig::default(),
        Arc::new(FixedMOTDProvider(None)),
        None,
    );

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
                    Err(err) if err.is_panic() => {
                        log::error!("panic from the listener");
                        std::panic::resume_unwind(err.into_panic());
                    },
                    Err(err)  => {
                        assert!(err.is_cancelled());
                        // otherwise, it's just an error due to cancellation of the task
                        // (when reloading the config)
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
