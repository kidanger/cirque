use cirque_core::ServerState;

use crate::session::Session;
use crate::transport::{AnyStream, Listener};
use crate::AnyListener;

fn handle_client(server_state: ServerState, stream: anyhow::Result<AnyStream>) {
    let fut = async move {
        // wait until we are in the async task to throw the error
        let mut stream = stream?;

        // enable the following to ease debugging:
        if false {
            stream = stream.with_debug();
        }

        Session::init(stream).run(server_state).await;
        log::info!("end of session a client");
        anyhow::Ok(())
    };

    tokio::spawn(async move {
        if let Err(err) = fut.await {
            log::error!("error when handling_client: {err}");
        }
    });
}

pub async fn run_server(listener: AnyListener, server_state: ServerState) -> anyhow::Result<()> {
    loop {
        let stream = listener.accept().await;
        handle_client(server_state.clone(), stream);
    }
}
