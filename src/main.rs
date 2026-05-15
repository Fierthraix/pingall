use std::io::{IsTerminal, stderr, stdout};

use pingall::cli_support::{
    PingBackend, can_open_raw_socket, command_exists, hostname_resolution_supported,
    raw_socket_supported, select_ping_backend,
};
use pingall::{ScanOptions, scan_each};

const HELP: &str = "\
Ping everything you can reach.

Usage: pingall [OPTIONS]

Options:
  -i, --interface <INTERFACE>  Interface to search
  -d, --dont-resolve           Don't attempt to resolve hostnames
      --no-resolve             Alias for --dont-resolve
  -r, --raw-socket             Open raw socket instead of using system `ping` command. Unix only, requires permissions
  -t, --timeout <TIMEOUT>      Timeout of pings in seconds [default: 1]
  -4, --ipv4                   Scan IPv4 addresses only
  -6, --ipv6                   Scan IPv6 addresses only
  -h, --help                   Print help
  -V, --version                Print version
";

#[derive(Debug)]
struct Args {
    interface: Option<String>,
    dont_resolve: bool,
    raw_socket: bool,
    timeout: usize,
    ipv4: bool,
    ipv6: bool,
}

impl Args {
    fn scan_ipv4(&self) -> bool {
        !self.ipv6
    }

    fn scan_ipv6(&self) -> bool {
        !self.ipv4
    }

    fn parse() -> Result<Self, Box<dyn std::error::Error>> {
        let mut args = pico_args::Arguments::from_env();

        if args.contains(["-h", "--help"]) {
            print!("{}", HELP);
            std::process::exit(0);
        }

        if args.contains(["-V", "--version"]) {
            println!("pingall {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }

        let dont_resolve = args.contains(["-d", "--dont-resolve"]);
        let no_resolve = args.contains("--no-resolve");

        let parsed = Self {
            interface: args.opt_value_from_str(["-i", "--interface"])?,
            dont_resolve: dont_resolve || no_resolve,
            raw_socket: args.contains(["-r", "--raw-socket"]),
            timeout: args.opt_value_from_str(["-t", "--timeout"])?.unwrap_or(1),
            ipv4: args.contains(["-4", "--ipv4"]),
            ipv6: args.contains(["-6", "--ipv6"]),
        };

        if parsed.ipv4 && parsed.ipv6 {
            return Err("the argument '--ipv4' cannot be used with '--ipv6'".into());
        }

        let remaining = args.finish();
        if !remaining.is_empty() {
            let unexpected = remaining
                .into_iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            return Err(format!("unexpected argument: {}", unexpected).into());
        }

        Ok(parsed)
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
    let args = Args::parse()?;

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
