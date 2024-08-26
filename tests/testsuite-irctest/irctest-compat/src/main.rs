use clap::Parser;

use cirque_core::{ServerState, WelcomeConfig};
use cirque_server::{AnyListener, TCPListener};

/// Simple program to greet a person
#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    port: u16,

    #[arg(short, long)]
    server_name: String,

    #[arg(long)]
    password: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let listener = TCPListener::try_new("[::]", args.port)?;

    let server_name = &args.server_name;
    let welcome_config = WelcomeConfig {
        send_isupport: true,
    };
    let motd = None;
    let password = args.password.map(|p| p.as_bytes().into());

    let server_state = ServerState::new(server_name, &welcome_config, motd, password);
    cirque_server::run_server(AnyListener::Tcp(listener), server_state).await
}
