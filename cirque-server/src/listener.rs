use std::sync::Arc;

use tokio::net::TcpListener;
use tokio_rustls::{
    rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer},
    },
    TlsAcceptor,
};

use crate::{connection_validator::ConnectionValidator, transport::AnyStream};

pub trait Listener {
    fn accept(
        &self,
        validator: &mut (impl ConnectionValidator + Send),
    ) -> impl std::future::Future<Output = std::io::Result<AnyStream>> + Send;
}

pub struct TCPListener {
    listener: TcpListener,
}

/// Bind a TCP socket from the std:: to be blocking (this function is not async),
/// then convert to a tokio:: listener for future use.
/// It has to be called within a tokio runtime with IO enabled.
fn bind_tcp_socket(addr: &str) -> std::io::Result<TcpListener> {
    let listener = std::net::TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    TcpListener::from_std(listener)
}

impl TCPListener {
    pub fn try_new(address: &str, port: u16) -> anyhow::Result<Self> {
        let addr = format!("{address}:{port}");
        let listener = bind_tcp_socket(&addr)?;

        log::info!("listening on {addr} (TCP without TLS)");
        Ok(Self { listener })
    }
}

impl Listener for TCPListener {
    async fn accept(
        &self,
        validator: &mut (impl ConnectionValidator + Send),
    ) -> std::io::Result<AnyStream> {
        let (stream, peer_addr) = self.listener.accept().await?;
        validator.validate(peer_addr)?;
        stream.set_nodelay(true)?;
        Ok(AnyStream::new(stream))
    }
}

pub struct TLSListener {
    listener: TcpListener,
    acceptor: TlsAcceptor,
}

impl TLSListener {
    pub fn try_new(
        address: &str,
        port: u16,
        certs: Vec<CertificateDer<'static>>,
        private_key: PrivateKeyDer<'static>,
    ) -> anyhow::Result<Self> {
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, private_key)?;

        let addr = format!("{address}:{port}");
        let listener = bind_tcp_socket(&addr)?;
        let acceptor = TlsAcceptor::from(Arc::new(config));

        log::info!("listening on {addr} (TCP with TLS)");
        Ok(Self { listener, acceptor })
    }
}

impl Listener for TLSListener {
    async fn accept(
        &self,
        validator: &mut (impl ConnectionValidator + Send),
    ) -> std::io::Result<AnyStream> {
        let (stream, peer_addr) = self.listener.accept().await?;
        validator.validate(peer_addr)?;
        let stream = self.acceptor.accept(stream).await?;
        Ok(AnyStream::new(stream))
    }
}
