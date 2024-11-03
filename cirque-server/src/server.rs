use cirque_core::ServerState;

use crate::connection_validator::ConnectionValidator;
use crate::listener::Listener;
use crate::session::run_session;
use crate::stream::Stream;

fn handle_client(server_state: ServerState, stream: std::io::Result<impl Stream + 'static>) {
    let stream = match stream {
        Ok(stream) => stream,
        Err(err) => {
            log::error!("error during acceptation with error: {err:#}");
            return;
        }
    };

    tokio::spawn(run_session(stream, server_state));
}

pub async fn run_server(
    listener: impl Listener,
    server_state: ServerState,
    mut connection_validator: impl ConnectionValidator + Send,
) -> ! {
    loop {
        let stream = listener.accept(&mut connection_validator).await;
        handle_client(server_state.clone(), stream);
    }
}
