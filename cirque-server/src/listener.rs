use std::net::SocketAddr;

use crate::stream::Stream;

pub use tcp::TCPListener;
pub use tls::TLSListener;

pub trait ConnectingStream {
    type Stream: Stream;

    fn handshake(self) -> impl std::future::Future<Output = std::io::Result<Self::Stream>> + Send;

    fn peer_addr(&self) -> SocketAddr;
}

pub trait Listener {
    type ConnectingStream: ConnectingStream + Send + 'static;

    fn accept(
        &self,
    ) -> impl std::future::Future<Output = std::io::Result<Self::ConnectingStream>> + Send;
}

mod tcp {
    use tokio::net::TcpListener;

    use super::{ConnectingStream, Listener};

    /// Bind a TCP socket from the std:: to be blocking (this function is not async),
    /// then convert to a tokio:: listener for future use.
    /// It has to be called within a tokio runtime with IO enabled.
    pub(crate) fn bind_tcp_socket(addr: &str) -> std::io::Result<TcpListener> {
        let listener = std::net::TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        TcpListener::from_std(listener)
    }

    pub struct TCPConnectingStream {
        stream: tokio::net::TcpStream,
        peer_addr: std::net::SocketAddr,
    }

    impl ConnectingStream for TCPConnectingStream {
        type Stream = tokio::net::TcpStream;

        async fn handshake(self) -> std::io::Result<Self::Stream> {
            Ok(self.stream)
        }

        fn peer_addr(&self) -> std::net::SocketAddr {
            self.peer_addr
        }
    }

    pub struct TCPListener {
        listener: TcpListener,
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
        type ConnectingStream = TCPConnectingStream;

        async fn accept(&self) -> std::io::Result<Self::ConnectingStream> {
            let (stream, peer_addr) = self.listener.accept().await?;
            stream.set_nodelay(true)?;

            Ok(TCPConnectingStream { stream, peer_addr })
        }
    }
}

mod tls {
    use tokio::net::TcpListener;

    use tokio_rustls::{
        TlsAcceptor,
        rustls::{
            self,
            pki_types::{CertificateDer, PrivateKeyDer},
        },
    };

    use super::tcp::bind_tcp_socket;
    use super::{ConnectingStream, Listener};

    pub struct TLSConnectingStream {
        stream: tokio::net::TcpStream,
        peer_addr: std::net::SocketAddr,
        acceptor: TlsAcceptor,
    }

    impl ConnectingStream for TLSConnectingStream {
        type Stream = tokio_rustls::server::TlsStream<tokio::net::TcpStream>;

        async fn handshake(self) -> std::io::Result<Self::Stream> {
            let stream = self.acceptor.accept(self.stream).await?;
            Ok(stream)
        }

        fn peer_addr(&self) -> std::net::SocketAddr {
            self.peer_addr
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
            let acceptor = TlsAcceptor::from(std::sync::Arc::new(config));

            log::info!("listening on {addr} (TCP with TLS)");
            Ok(Self { listener, acceptor })
        }
    }

    impl Listener for TLSListener {
        type ConnectingStream = TLSConnectingStream;

        async fn accept(&self) -> std::io::Result<Self::ConnectingStream> {
            let (stream, peer_addr) = self.listener.accept().await?;

            Ok(TLSConnectingStream {
                stream,
                peer_addr,
                acceptor: self.acceptor.clone(),
            })
        }
    }
}
