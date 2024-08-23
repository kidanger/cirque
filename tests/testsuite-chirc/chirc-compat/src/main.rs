use std::{io::Read, sync::Arc};

use clap::Parser;

use cirque_core::{ServerState, WelcomeConfig};
use cirque_server::{AnyListener, TCPListener};

/// Simple program to greet a person
#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    port: u16,

    #[arg(short, long)]
    oper_password: Option<String>,
}

struct FileMOTDProvider {
    filename: String,
}

impl cirque_core::MOTDProvider for FileMOTDProvider {
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

    let server_name = "srv";
    let welcome_config = WelcomeConfig {
        send_isupport: false,
    };
    let motd_provider = Arc::new(FileMOTDProvider {
        filename: "motd.txt".to_string(),
    });

    let server_state = ServerState::new(server_name, &welcome_config, motd_provider, None);
    cirque_server::run_server(AnyListener::Tcp(listener), server_state).await
}
