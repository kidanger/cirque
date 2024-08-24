use tokio::io::{AsyncReadExt, AsyncWriteExt};

use cirque_core::ServerState;
use cirque_core::UserState;
use cirque_parser::{LendingIterator, StreamParser};

use crate::transport::AnyStream;

pub(crate) struct Session {
    stream: AnyStream,
}

impl Session {
    pub(crate) fn init(stream: AnyStream) -> Self {
        Self { stream }
    }

    // TODO: remove the Result, we don't want shortcuts here
    pub(crate) async fn run(mut self, server_state: ServerState) -> anyhow::Result<()> {
        let mut stream_parser = StreamParser::default();

        let (user_id, mut state, mut rx) = server_state.new_registering_user();

        while !state.client_disconnected_voluntarily() {
            tokio::select! {
                result = self.stream.read_buf(&mut stream_parser) => {
                    let received = result?;

                    if received == 0 {
                        break;
                    }

                    let mut iter = stream_parser.consume_iter();
                    while let Some(message) = iter.next() {
                        let message = match message {
                            Ok(m) => m,
                            Err(_err) => {
                                // TODO: log
                                continue;
                            }
                        };

                        state = state.handle_message(&server_state, message);
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
        } else if let UserState::Registering(state) = state {
            // the connection was closed without notification
            server_state.ruser_disconnects_suddently(state);
        } else if let UserState::Registered(_state) = state {
            // the connection was closed without notification
            server_state.user_disconnects_suddently(user_id);
        }

        Ok(())
    }
}
