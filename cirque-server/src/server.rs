use cirque_core::SharedServerState;

use crate::session::Session;
use crate::transport::{AnyStream, Listener};
use crate::AnyListener;

fn handle_client(server_state: SharedServerState, stream: anyhow::Result<AnyStream>) {
    let fut = async move {
        // wait until we are in the async task to throw the error
        let stream = stream?;
        let stream = stream.with_debug();
        Session::init(stream).run(server_state).await?;
        // TODO: log
        anyhow::Ok(())
    };

    tokio::spawn(async move {
        if let Err(_err) = fut.await {
            // TODO: log
            //eprintln!("{:?}", err);
        }
    });
}

pub async fn run_server(
    listener: AnyListener,
    server_state: SharedServerState,
) -> anyhow::Result<()> {
    loop {
        let stream = listener.accept().await;
        handle_client(server_state.clone(), stream);
    }
}
