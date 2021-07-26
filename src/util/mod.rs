use std::net::{IpAddr, Ipv4Addr};
use std::process::Stdio;
use std::time::Duration;

use nix::ifaddrs::getifaddrs;
use nix::sys::socket;
use structopt::StructOpt;
use surge_ping::Pinger;
use tokio::process::Command;

#[derive(StructOpt, Debug)]
pub(crate) struct Opt {
    /// Interface to search.
    #[structopt[short = "i", long = "interface"]]
    pub(crate) interface: Option<String>,

    /// Don't attempt to resolve hostnames.
    #[structopt(short = "d", long = "dont-resolve")]
    pub(crate) dont_resolve: bool,

    /// Open raw socket instead of using system `ping` command.
    #[structopt(short = "r", long = "raw-socket")]
    pub(crate) raw_socket: bool,
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
    let filter_ip = |address| {
        if let Some(socket::SockAddr::Inet(inet_addr)) = address {
            if let socket::IpAddr::V4(ip_addr) = inet_addr.ip() {
                let std_ip = ip_addr.to_std();
                if std_ip != Ipv4Addr::LOCALHOST {
                    Some(std_ip)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
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
pub(crate) async fn system_ping(ip_addr: &IpAddr) -> bool {
    let mut command = Command::new("ping")
        .arg("-W")
        .arg("1")
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

pub(crate) async fn socket_ping(ip_addr: &IpAddr) -> bool {
    let mut pinger = if let Ok(pinger) = Pinger::new(*ip_addr) {
        pinger
    } else {
        return false;
    };

    pinger.timeout(Duration::from_secs(1));

    pinger.ping(0).await.is_ok()
}
