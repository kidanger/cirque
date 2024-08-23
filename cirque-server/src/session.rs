use tokio::io::{AsyncReadExt, AsyncWriteExt};

use cirque_core::RegisteringState;
use cirque_core::SessionState;
use cirque_core::SharedServerState;
use cirque_parser::{LendingIterator, MessageIteratorError, StreamParser};

use crate::transport::AnyStream;

pub(crate) struct Session {
    stream: AnyStream,
}

impl Session {
    pub(crate) fn init(stream: AnyStream) -> Self {
        Self { stream }
    }

    pub(crate) async fn run(mut self, server_state: SharedServerState) -> anyhow::Result<()> {
        let mut stream_parser = StreamParser::default();

        let (user_id, mut rx) = server_state.lock().unwrap().new_registering_user();
        let mut state = SessionState::Registering(RegisteringState::new(user_id));

        while !state.client_disconnected_voluntarily() {
            tokio::select! {
                result = self.stream.read_buf(&mut stream_parser) => {
                    let received = result?;

                    if received == 0 {
                        break;
                    }

                    let mut iter = stream_parser.consume_iter();
                    let mut reset_buffer = false;
                    while let Some(message) = iter.next() {
                        let message = match message {
                            Ok(m) => m,
                            Err(MessageIteratorError::BufferFullWithoutMessageError) => {
                                reset_buffer = true;
                                break;
                            }
                            Err(MessageIteratorError::ParsingError(_e)) => {
                                // TODO: log
                                continue;
                            }
                        };

                        let mut server_state = server_state.lock().unwrap();
                        state = state.handle_message(&mut server_state, message);
                    }
                    if reset_buffer {
                        stream_parser.clear();
                    }
                },
                Some(msg) = rx.recv() => {
                    self.stream.write_all(&msg).await?;
                }
            }
        }

        if state.client_disconnected_voluntarily() {
            // the client sent a QUIT, handle the disconnection gracefully by sending remaining
            // messages
            let mut buf = std::io::Cursor::new(Vec::<u8>::new());
            while let Ok(msg) = rx.try_recv() {
                std::io::Write::write_all(&mut buf, &msg)?;
            }
            // TODO: maybe tolerate a timeout to send the last messages and then force quit
            self.stream.write_all(&buf.into_inner()).await?;
            //self.stream.flush().await?;
        } else if let SessionState::Registering(_) = state {
            // the connection was closed without notification
            server_state
                .lock()
                .unwrap()
                .ruser_disconnects_suddently(user_id);
        } else if let SessionState::Registered(_) = state {
            // the connection was closed without notification
            server_state
                .lock()
                .unwrap()
                .user_disconnects_suddently(user_id);
        }

        Ok(())
    }
}
