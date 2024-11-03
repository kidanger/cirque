use std::io::Write;

use tokio::io::{AsyncRead, AsyncWrite};

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
