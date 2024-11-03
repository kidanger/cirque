use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}

impl Stream for TcpStream {}
impl Stream for tokio_rustls::server::TlsStream<TcpStream> {}
