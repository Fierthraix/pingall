use std::net::Ipv4Addr;
use std::process::{Command, Stdio};

use nix::ifaddrs::getifaddrs;
use nix::sys::socket;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
pub(crate) struct Opt {
    /// Interface to search.
    #[structopt[short = "i", long = "interface"]]
    pub(crate) interface: Option<String>,

    /// Don't attempt to resolve hostnames.
    #[structopt(short = "d", long = "dont-resolve")]
    pub(crate) dont_resolve: bool,
}

/// Check if a command is available in the current `$PATH`.
pub(crate) fn command_exists(command: &str) -> bool {
    let command = Command::new("which")
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
