use std::{io::Read, sync::Arc};

use cirque::{ServerState, TCPListener};
use clap::Parser;

/// Simple program to greet a person
#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    port: u16,

    #[arg(short, long)]
    server_name: String,
}

struct FileMOTDProvider {
    filename: String,
}

impl cirque::MOTDProvider for FileMOTDProvider {
    fn motd(&self) -> Option<Vec<Vec<u8>>> {
        let Ok(mut file) = std::fs::File::open(&self.filename) else {
            return None;
        };
        let mut content = String::new();
        file.read_to_string(&mut content).ok()?;
        let lines = content.lines().map(|l| l.as_bytes().to_vec()).collect();
        Some(lines)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let listener = TCPListener::try_new(args.port).await?;

    let motd_provider = Arc::new(FileMOTDProvider {
        filename: "motd.txt".to_string(),
    });

    let server_name = &args.server_name;

    let server_state = ServerState::new(server_name, motd_provider);
    cirque::run_server(listener, server_state).await
}
