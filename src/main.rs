use std::io::{IsTerminal, stderr, stdout};

use clap::{ArgAction, Parser};
use pingall::cli_support::{
    PingBackend, can_open_raw_socket, command_exists, hostname_resolution_supported,
    raw_socket_supported, select_ping_backend,
};
use pingall::{ScanOptions, scan_each};

#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// Interface to search.
    #[arg(short, long)]
    interface: Option<String>,

    /// Don't attempt to resolve hostnames.
    #[arg(short = 'd', long = "dont-resolve", alias = "no-resolve", action = ArgAction::SetTrue)]
    dont_resolve: bool,

    /// Open raw socket instead of using system `ping` command. Unix only, requires permissions.
    #[arg(short = 'r', long = "raw-socket", action = ArgAction::SetTrue)]
    raw_socket: bool,

    /// Timeout of pings in seconds.
    #[arg(short, long, default_value_t = 1)]
    timeout: usize,

    /// Scan IPv4 addresses only.
    #[arg(short = '4', long = "ipv4", conflicts_with = "ipv6", action = ArgAction::SetTrue)]
    ipv4: bool,

    /// Scan IPv6 addresses only.
    #[arg(short = '6', long = "ipv6", conflicts_with = "ipv4", action = ArgAction::SetTrue)]
    ipv6: bool,
}

impl Args {
    fn scan_ipv4(&self) -> bool {
        !self.ipv6
    }

    fn scan_ipv6(&self) -> bool {
        !self.ipv4
    }
}

fn main() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            if let Err(e) = run().await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        })
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let resolve_hostnames = if args.dont_resolve {
        false
    } else if hostname_resolution_supported() {
        true
    } else {
        if cfg!(target_os = "linux") && stdout().is_terminal() {
            eprintln!("`avahi-resolve` not found, hostname resolution disabled");
        }
        false
    };

    if args.raw_socket && !raw_socket_supported() && stderr().is_terminal() {
        eprintln!(
            "Raw socket mode is unsupported on this platform; falling back to system `ping`."
        );
    }

    let system_ping_exists = command_exists("ping");
    if args.scan_ipv6() && !system_ping_exists {
        return Err("system `ping` command not found and IPv6 discovery requires it".into());
    }

    let ping_backend = select_ping_backend(args.raw_socket, system_ping_exists)?;
    if args.scan_ipv4()
        && ping_backend == PingBackend::RawSocket
        && !can_open_raw_socket().await
        && stderr().is_terminal()
    {
        let err_msg = "Either run as root, or run `setcap cap_net_raw+ep $(which pingall)` to allow this app to open raw sockets.";
        eprintln!("Error opening raw socket.\n{}", err_msg);
    }

    let ipv4 = args.scan_ipv4();
    let ipv6 = args.scan_ipv6();
    let options = ScanOptions {
        interface: args.interface,
        resolve_hostnames,
        raw_socket: args.raw_socket,
        timeout: args.timeout,
        ipv4,
        ipv6,
    };

    scan_each(options, |result| println!("{}", result)).await?;

    Ok(())
}
