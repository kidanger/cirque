use crate::server_state::ServerState;
use crate::session::Session;
use crate::transport::Listener;

pub async fn run_server(listener: impl Listener, server_state: ServerState) -> anyhow::Result<()> {
    let server_state = server_state.shared();

    loop {
        let server_state = server_state.clone();
        let stream = listener.accept().await;
        let fut = async move {
            let stream = stream?;
            let stream = stream.with_debug();
            Session::init(stream).run(server_state).await?;
            dbg!("client dropped");
            anyhow::Ok(())
        };

        tokio::spawn(async move {
            if let Err(err) = fut.await {
                eprintln!("{:?}", err);
            }
        });
    }
}
