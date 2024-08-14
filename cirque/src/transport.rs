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

pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}

impl Stream for tokio::net::TcpStream {}
impl Stream for tokio_rustls::server::TlsStream<tokio::net::TcpStream> {}

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
    async fn accept(&self) -> anyhow::Result<AnyStream>;
}

pub(crate) struct TCPListener {
    listener: TcpListener,
}

impl TCPListener {
    pub async fn try_new(port: u16) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(format!("[::]:{}", port)).await?;
        Ok(Self { listener })
    }
}

impl Listener for TCPListener {
    async fn accept(&self) -> anyhow::Result<AnyStream> {
        let (stream, _peer_addr) = self.listener.accept().await?;
        Ok(AnyStream::new(stream))
    }
}

pub(crate) struct TLSListener {
    listener: TcpListener,
    acceptor: TlsAcceptor,
}

impl TLSListener {
    pub async fn try_new(
        certs: Vec<CertificateDer<'static>>,
        private_key: PrivateKeyDer<'static>,
    ) -> anyhow::Result<Self> {
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, private_key)?;

        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind(format!("[::]:{}", 6697)).await?;
        Ok(Self { listener, acceptor })
    }
}

impl Listener for TLSListener {
    async fn accept(&self) -> anyhow::Result<AnyStream> {
        let (stream, _peer_addr) = self.listener.accept().await?;
        let stream = self.acceptor.accept(stream).await?;
        Ok(AnyStream::new(stream))
    }
}