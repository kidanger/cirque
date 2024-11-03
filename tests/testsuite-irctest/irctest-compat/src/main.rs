use clap::Parser;

use cirque_core::{ServerState, WelcomeConfig};
use cirque_server::{AcceptAll, TCPListener};

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

    let server_state = ServerState::new(server_name, &welcome_config, motd, password, None);
    server_state.set_messages_per_second_limit(100);
    cirque_server::run_server(listener, server_state, AcceptAll {}).await
}
