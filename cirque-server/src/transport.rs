use std::{io::Write, sync::Arc};

use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
};
use tokio_rustls::{
    rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer},
    },
    TlsAcceptor,
};

use crate::connection_validator::ConnectionValidator;

pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}

impl Stream for tokio::net::TcpStream {}
impl Stream for tokio_rustls::server::TlsStream<tokio::net::TcpStream> {}
impl Stream for std::io::Cursor<Vec<u8>> {}

pub struct AnyStream {
    inner: Box<dyn Stream>,
    debug: bool,
}

impl AnyStream {
    pub fn new<S: Stream + 'static>(inner: S) -> Self {
        Self {
            inner: Box::new(inner),
            debug: false,
        }
    }

    pub fn with_debug(self) -> Self {
        Self {
            inner: self.inner,
            debug: true,
        }
    }
}

impl Stream for AnyStream {}

impl AsyncRead for AnyStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let mut pinned = std::pin::pin!(&mut self.inner);
        let start = buf.filled().len();
        let result = pinned.as_mut().poll_read(cx, buf);
        if self.debug {
            #[allow(clippy::indexing_slicing)]
            std::io::stdout().write_all(&buf.filled()[start..])?;
        }
        result
    }
}

impl AsyncWrite for AnyStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        if self.debug {
            std::io::stdout().write_all(buf)?;
        }
        let mut pinned = std::pin::pin!(&mut self.inner);
        pinned.as_mut().poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        if self.debug {
            std::io::stdout().flush()?;
        }
        let mut pinned = std::pin::pin!(&mut self.inner);
        pinned.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        let mut pinned = std::pin::pin!(&mut self.inner);
        pinned.as_mut().poll_shutdown(cx)
    }
}

pub trait Listener {
    fn accept<CV>(
        &self,
        validator: &mut CV,
    ) -> impl std::future::Future<Output = std::io::Result<AnyStream>> + Send
    where
        CV: ConnectionValidator + Send;
}

pub struct TCPListener {
    listener: TcpListener,
}

impl TCPListener {
    pub fn try_new(address: &str, port: u16) -> anyhow::Result<Self> {
        let listener = std::net::TcpListener::bind(format!("{address}:{port}"))?;
        listener.set_nonblocking(true)?;
        let listener = TcpListener::from_std(listener)?;
        Ok(Self { listener })
    }
}

impl Listener for TCPListener {
    async fn accept<CV>(&self, validator: &mut CV) -> std::io::Result<AnyStream>
    where
        CV: ConnectionValidator,
    {
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

        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = std::net::TcpListener::bind(format!("{address}:{port}"))?;
        listener.set_nonblocking(true)?;
        let listener = TcpListener::from_std(listener)?;
        Ok(Self { listener, acceptor })
    }

    pub fn update_keys(
        &mut self,
        certs: Vec<CertificateDer<'static>>,
        private_key: PrivateKeyDer<'static>,
    ) -> anyhow::Result<()> {
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, private_key)?;

        self.acceptor = TlsAcceptor::from(Arc::new(config));
        Ok(())
    }
}

impl Listener for TLSListener {
    async fn accept<CV>(&self, validator: &mut CV) -> std::io::Result<AnyStream>
    where
        CV: ConnectionValidator + Send,
    {
        let (stream, peer_addr) = self.listener.accept().await?;
        validator.validate(peer_addr)?;
        let stream = self.acceptor.accept(stream).await?;
        Ok(AnyStream::new(stream))
    }
}

pub enum AnyListener {
    Tls(TLSListener),
    Tcp(TCPListener),
}

impl Listener for AnyListener {
    async fn accept<CV>(&self, validator: &mut CV) -> std::io::Result<AnyStream>
    where
        CV: ConnectionValidator + Send,
    {
        match self {
            AnyListener::Tls(l) => l.accept(validator).await,
            AnyListener::Tcp(l) => l.accept(validator).await,
        }
    }
}
