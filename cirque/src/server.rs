use crate::server_state::ServerState;
use crate::session::Session;
use crate::transport::Listener;

pub async fn run_server(listener: impl Listener, server_state: ServerState) -> anyhow::Result<()> {
    let server_state = server_state.shared();

    loop {
        let stream = listener.accept().await?;
        let stream = stream.with_debug();

        let server_state = server_state.clone();
        let fut = async move {
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
