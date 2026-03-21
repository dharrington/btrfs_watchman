mod server;
mod snapshot_manager;
mod watchman_protocol;
mod evaluator;#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        #[cfg(feature = "debug_logging")]
        eprintln!("[server] {}", format_args!($($arg)*));
    };
}

use anyhow::Result;
use std::sync::Arc;
use tokio::net::UnixListener;

use server::{handle_client, ServerState};
use watchman_protocol::GetSockNameResponse;

fn print_usage() {
    eprintln!("btrfs_watchman - A Watchman-compatible server using Btrfs snapshots");
    eprintln!();
    eprintln!("Usage: btrfs_watchman [options] [socket_path]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --output-encoding <enc>    Reserved for compatibility (e.g. bser-v2)");
    eprintln!("  get-sockname               Print the socket path and exit");
    eprintln!("  -h, --help                 Print this help message");
    eprintln!();
    eprintln!("Socket path defaults to a temp file (e.g. /tmp/btrfs_watchman.sock) if not specified.");
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    
    let mut output_encoding = "json";
    let mut command = None;
    let user = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    let default_sock = std::env::temp_dir()
        .join(format!("btrfs_watchman_{}.sock", user))
        .to_string_lossy()
        .into_owned();
    let mut socket_path = default_sock;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--output-encoding" => {
                if i + 1 < args.len() {
                    output_encoding = &args[i + 1];
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "get-sockname" => {
                command = Some("get-sockname");
                i += 1;
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            s if i == 1 && !s.starts_with('-') => {
                socket_path = s.to_string();
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    if command == Some("get-sockname") {
        let abs_socket_path = if let Ok(abs) = std::fs::canonicalize(&socket_path) {
            abs
        } else {
            std::env::current_dir()?.join(&socket_path)
        };
        
        // Try starting the server if it's not already running
        if tokio::net::UnixStream::connect(&abs_socket_path).await.is_err() {
            let log_file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("/tmp/btrfs_watchman.log")?;
                
            let exe = std::env::current_exe()?;
            std::process::Command::new(exe)
                .arg(&abs_socket_path)
                .stdout(log_file.try_clone()?)
                .stderr(log_file)
                .spawn()?;
            // Give it a moment to bind the socket
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }

        let response = GetSockNameResponse {
            version: "btrfs-watchman-0.1.0".to_string(),
            sockname: Some(abs_socket_path),
            error: None,
        };

        match output_encoding {
            "bser-v2" => {
                let mut stdout = std::io::stdout();
                serde_bser::ser::serialize(&mut stdout, &response)?;
            }
            _ => {
                println!("{}", serde_json::to_string(&response)?);
            }
        }
        return Ok(());
    }

    if std::fs::metadata(&socket_path).is_ok() {
        std::fs::remove_file(&socket_path)?;
    }

    let listener = UnixListener::bind(&socket_path)?;
    println!("btrfs-watchman server listening on {}", socket_path);

    let state = Arc::new(ServerState::new()?);

    loop {
        let (socket, addr) = listener.accept().await?;
        println!("Client connected: {:?}", addr);
        let state_clone = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(socket, state_clone).await {
                eprintln!("Error handling client: {}", e);
            }
        });
    }
}
