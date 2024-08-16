use cirque::TCPListener;
use clap::Parser;

/// Simple program to greet a person
#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    port: u16,

    #[arg(short, long)]
    oper_password: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let listener = TCPListener::try_new(args.port).await?;
    let server_state = Default::default();
    cirque::run_server(listener, server_state).await
}
