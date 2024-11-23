use cirque_core::ServerState;

use crate::connection_validator::ConnectionValidator;
use crate::listener::ConnectingStream;
use crate::listener::Listener;
use crate::session::run_session;

async fn handle_client(server_state: ServerState, connecting_stream: impl ConnectingStream) {
    let stream = connecting_stream.handshake().await;

    let stream = match stream {
        Ok(stream) => stream,
        Err(err) => {
            log::error!("error during connection handshake with error: {err:#}");
            return;
        }
    };

    run_session(stream, server_state).await;
}

pub async fn run_server(
    listener: impl Listener,
    server_state: ServerState,
    mut connection_validator: impl ConnectionValidator + Send,
) -> ! {
    loop {
        let conn = listener.accept().await;
        let conn = conn.and_then(|c| connection_validator.validate(c.peer_addr()).map(|_| c));

        let conn = match conn {
            Ok(connecting_stream) => connecting_stream,
            Err(err) => {
                log::error!("error during connection acceptation/validation with error: {err:#}");
                continue;
            }
        };

        tokio::spawn(handle_client(server_state.clone(), conn));
    }
}
