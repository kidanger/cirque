use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
mod config;
use anyhow::{anyhow, Ok};
use std::{path::PathBuf, str::FromStr};
use tokio::net::TcpListener;
use tokio_rustls::{rustls, TlsAcceptor};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let config_path = PathBuf::from_str("ircd.yml")?;
    let c = config::Config::new(&config_path)?;

    let mut certs = None;
    if let Some(cert_file_path) = c.cert_file_path {
        certs = Some(
            rustls_pemfile::certs(&mut BufReader::new(&mut File::open(cert_file_path)?))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    let mut private_key = None;
    if let Some(private_key_file_path) = c.private_key_file_path {
        private_key = rustls_pemfile::private_key(&mut BufReader::new(&mut File::open(
            private_key_file_path,
        )?))?;
    }

    if certs.is_some() && private_key.is_some() {
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs.unwrap(), private_key.unwrap())?;

        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind(format!("[::]:{}", 6697)).await?;
        loop {
            let (stream, _peer_addr) = listener.accept().await?;
            let acceptor = acceptor.clone();

            let fut = async move {
                let mut _stream = acceptor.accept(stream).await?;
                Ok(())
            };

            tokio::spawn(async move {
                if let Err(err) = fut.await {
                    eprintln!("{:?}", err);
                }
            });
        }
    } else {
        Err(anyhow!("Config incomplete"))
    }
}
