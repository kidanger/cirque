use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use cirque_core::ServerState;
use cirque_parser::{LendingIterator, StreamParser};

use crate::message_throttler::MessageThrottler;
use crate::stream::Stream;

pub(crate) async fn run_session(mut stream: impl Stream, server_state: ServerState) {
    let mut stream_parser = StreamParser::default();
    let mut message_throttler = MessageThrottler::new(server_state.get_messages_per_second_limit());

    let timeout = server_state
        .get_timeout()
        .unwrap_or_else(|| Duration::from_secs(99999));
    let mut timer = tokio::time::interval(timeout.div_f32(4.));

    let (mut state, mut rx) = server_state.new_registering_user();

    while state.is_alive() {
        tokio::select! {
            result = stream.read_buf(&mut stream_parser) => {
                let Ok(received) = result else {
                    break;
                };

                if received == 0 {
                    break;
                }

                let mut iter = stream_parser.consume_iter();
                while let Some(message) = iter.next() {
                    let message = match message {
                        Ok(m) => m,
                        Err(err) => {
                            log::warn!("error when parsing message: {err:#}");
                            continue;
                        }
                    };

                    state = state.handle_message(&server_state, message);
                    message_throttler.maybe_slow_down().await;
                }
            },
            msg = rx.recv() => {
                if let Some(msg) = msg {
                    if stream.write_all(&msg).await.is_err() {
                        break;
                    }
                } else {
                    // mailbox sender was closed, probably because of
                    // RegisteringUser/RegisteredUser was dropped.
                    break;
                }
            }
            _ = timer.tick() => {
                state = state.check_timeout(&server_state);
            }
        }
    }

    server_state.dispose_state(state);
    // close the mailbox, we don't want to receive any more messages at this point
    rx.close();

    // handle the disconnection gracefully by sending remaining
    // messages (in case the client asked a QUIT for example)
    let buf = {
        let mut buf = std::io::Cursor::new(Vec::<u8>::new());
        while let Ok(msg) = rx.try_recv() {
            let _ = std::io::Write::write_all(&mut buf, &msg);
        }
        buf.into_inner()
    };
    // try to send the messages, but don't hang on the client just for theses
    let _ = tokio::time::timeout(Duration::from_secs(10), stream.write_all(&buf)).await;
}
