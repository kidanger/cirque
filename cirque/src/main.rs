use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
mod config;
use std::{path::PathBuf, str::FromStr};

use anyhow::{anyhow, Ok};

fn main() -> Result<(), anyhow::Error> {
    let config_path = PathBuf::from_str("default.yml")?;
    let c = config::Config::new(&config_path)?;

    let mut certs = None;
    if let Some(cert_file_path) = c.cert_file_path {
        certs = Some(rustls_pemfile::certs(&mut BufReader::new(&mut File::open(cert_file_path)?))
            .collect::<Result<Vec<_>, _>>()?);
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

        let listener = TcpListener::bind(format!("[::]:{}", 4443)).unwrap();
        let (mut stream, _) = listener.accept()?;

        let mut conn = rustls::ServerConnection::new(Arc::new(config))?;
        conn.complete_io(&mut stream)?;

        conn.writer().write_all(b"Hello from the server")?;
        conn.complete_io(&mut stream)?;
        let mut buf = [0; 64];
        let len = conn.reader().read(&mut buf)?;
        println!("Received message from client: {:?}", &buf[..len]);

        return Ok(());
    }
    else {
        Err(anyhow!("Config incomplete"))
    }

}
