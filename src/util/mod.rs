use std::net::{IpAddr, Ipv4Addr};
use std::process::Stdio;
use std::time::Duration;

use nix::ifaddrs::getifaddrs;
use nix::sys::socket;
use tokio::process::Command;

use tiny_ping::Pinger;

const HELP: &str = r#"
USAGE:
    pingall [FLAGS]

FLAGS:
    -i <interface>        Interface to search
    -d, --dont-resolve    Don't attempt to resolve hostnames
    -h, --help            Prints help information
    -r, --raw-socket      Open raw socket instead of using system `ping` command. Requires permissions
    -t, --timeout         Timeout of pings in seconds (default 1)
    "#;

#[derive(Debug)]
pub(crate) struct Args {
    pub(crate) interface: Option<String>,

    pub(crate) dont_resolve: bool,

    pub(crate) raw_socket: bool,

    pub(crate) timeout: usize,
}

pub(crate) fn get_args() -> Args {
    let mut pargs = pico_args::Arguments::from_env();

    if pargs.contains(["-h", "--help"]) {
        print!("{}", HELP);
        std::process::exit(0);
    }

    Args {
        interface: pargs.opt_value_from_str("-i").unwrap(),
        dont_resolve: pargs.contains(["-d", "--dont-resolve"]),
        raw_socket: pargs.contains(["-r", "--raw-socket"]),
        timeout: pargs
            .value_from_fn(["-t", "--timeout"], str::parse)
            .unwrap_or(1),
    }
}

/// Check if a command is available in the current `$PATH`.
pub(crate) fn command_exists(command: &str) -> bool {
    let command = std::process::Command::new("which")
        .arg(command)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match command {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

/// List the IPv4 address associated with an interface.
/// Given no interface, list all non-loopback IP addresses of all interfaces.
pub(crate) fn get_addresses(interface: Option<String>) -> Vec<Ipv4Addr> {
    // Try to convert `nix::ifaddrs::InterfaceAddress` to `std::net::Ipv4Addr`.
    let filter_ip = |wrapped_address: Option<socket::SockaddrStorage>| {
        if let Some(address) = wrapped_address {
            if let Some(sock_addr) = address.as_sockaddr_in() {
                let ip_addr = sock_addr.ip();
                if ip_addr != Ipv4Addr::LOCALHOST {
                    return Some(ip_addr);
                }
            };
        };
        None
    };

    // Interface supplied, only check it.
    if let Some(interface) = interface {
        getifaddrs()
            .unwrap()
            .filter_map(|ifaddr| {
                if ifaddr.interface_name == interface && ifaddr.address.is_some() {
                    filter_ip(ifaddr.address)
                } else {
                    None
                }
            })
            .take(1)
            .collect()
    } else {
        // Get ip addrs of all interfaces.
        getifaddrs()
            .unwrap()
            .filter_map(|ifaddr| filter_ip(ifaddr.address))
            .collect()
    }
}

/// Ping using system `ping` command.
pub(crate) async fn system_ping(ip_addr: &IpAddr, timeout: usize) -> bool {
    let mut command = Command::new("ping")
        .arg("-W")
        .arg(timeout.to_string())
        .arg("-c")
        .arg("1")
        .arg(ip_addr.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn");

    // Check if the ping succeeded.
    match command.wait().await {
        Ok(status) => status.code().unwrap_or(1) == 0,
        Err(_) => false,
    }
}

pub(crate) async fn socket_ping(ip_addr: &IpAddr, timeout: usize) -> bool {
    if let Ok(mut pinger) = Pinger::new(*ip_addr) {
        pinger.timeout(Duration::from_secs(timeout as u64));
        return pinger.ping(0).await.is_ok();
    }
    false
}

pub(crate) async fn can_open_raw_socket() -> bool {
    let localhost = IpAddr::V4(Ipv4Addr::LOCALHOST);
    if let Ok(mut pinger) = Pinger::new(localhost) {
        pinger.timeout(Duration::from_secs(1));
        return pinger.ping(0).await.is_ok();
    }
    false
}
