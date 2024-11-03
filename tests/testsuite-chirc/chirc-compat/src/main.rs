use clap::Parser;

use cirque_core::{ServerState, WelcomeConfig};
use cirque_server::{AcceptAll, TCPListener};

/// Simple program to greet a person
#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    port: u16,

    #[arg(short, long)]
    oper_password: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let listener = TCPListener::try_new("[::]", args.port)?;

    let server_name = "srv";
    let welcome_config = WelcomeConfig {
        send_isupport: false,
    };
    let motd = None;

    let server_state = ServerState::new(server_name, &welcome_config, motd, None, None);
    server_state.set_messages_per_second_limit(100);
    cirque_server::run_server(listener, server_state, AcceptAll {}).await
}
