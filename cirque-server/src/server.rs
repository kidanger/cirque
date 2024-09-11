use cirque_core::ServerState;

use crate::connection_validator::ConnectionValidator;
use crate::session::Session;
use crate::transport::{AnyStream, Listener};
use crate::AnyListener;

fn handle_client(server_state: ServerState, stream: std::io::Result<AnyStream>) {
    let fut = async move {
        // wait until we are in the async task to throw the error
        let mut stream = stream?;

        // enable the following to ease debugging:
        if false {
            stream = stream.with_debug();
        }

        Session::init(stream).run(server_state).await;
        anyhow::Ok(())
    };

    tokio::spawn(async move {
        if let Err(err) = fut.await {
            log::error!("session closed with error: {err:#}");
        }
    });
}

pub async fn run_server(
    listener: AnyListener,
    server_state: ServerState,
    mut connection_validator: impl ConnectionValidator + Send,
) -> ! {
    loop {
        let stream = listener.accept(&mut connection_validator).await;
        handle_client(server_state.clone(), stream);
    }
}
