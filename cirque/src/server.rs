use crate::server_state::SharedServerState;
use crate::session::ConnectingSession;
use crate::transport::Listener;

pub async fn run_server(
    listener: impl Listener,
    server_state: SharedServerState,
) -> anyhow::Result<()> {
    loop {
        let stream = listener.accept().await?;
        let stream = stream.with_debug();

        let server_state = server_state.clone();
        let fut = async move {
            let session = ConnectingSession::new(stream);
            let (session, user) = session.connect_user(&server_state).await?;
            server_state.lock().unwrap().add_user(user);
            session.run(server_state).await?;
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
