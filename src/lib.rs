//! Minimal library API for scanning the local network.
//!
//! `pingall` is primarily a command-line tool. The library API intentionally
//! mirrors that tool's scan operation without exposing the lower-level probing
//! implementation details.

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

mod util;

use util::{
    DiscoveredAddress, InterfaceAddress, PingBackend, get_addresses, hostname_resolution_supported,
    resolve_hostname, select_ping_backend, socket_ping, system_ipv6_multicast_ping, system_ping,
};

/// Options for a local network scan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanOptions {
    /// Interface to search. When unset, all non-loopback interfaces are scanned.
    pub interface: Option<String>,
    /// Attempt to resolve hostnames for responding addresses.
    pub resolve_hostnames: bool,
    /// Open raw sockets instead of using the system `ping` command where supported.
    pub raw_socket: bool,
    /// Timeout of pings in seconds.
    pub timeout: usize,
    /// Scan IPv4 addresses.
    pub ipv4: bool,
    /// Scan IPv6 addresses.
    pub ipv6: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            interface: None,
            resolve_hostnames: true,
            raw_socket: false,
            timeout: 1,
            ipv4: true,
            ipv6: true,
        }
    }
}

/// Scan the local network and return the lines normally printed by the CLI.
///
/// Results are deduplicated and formatted as either `IP` or `IP<TAB>hostname`,
/// depending on whether hostname resolution is requested and succeeds.
pub async fn scan(options: ScanOptions) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut results = Vec::new();
    scan_each(options, |result| results.push(result)).await?;
    Ok(results)
}

/// Scan the local network and call `on_result` as each result becomes available.
///
/// Results are deduplicated before they are passed to the callback. The callback
/// receives the same formatted lines returned by [`scan`].
pub async fn scan_each<F>(
    options: ScanOptions,
    mut on_result: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut(String),
{
    let resolve = options.resolve_hostnames && hostname_resolution_supported();
    let system_ping_exists = util::command_exists("ping");

    if options.ipv6 && !system_ping_exists {
        return Err("system `ping` command not found and IPv6 discovery requires it".into());
    }

    let ping_backend = select_ping_backend(options.raw_socket, system_ping_exists)?;
    let addresses = get_addresses(options.interface);
    let semaphore = Arc::new(Semaphore::new(150));

    let mut tasks = JoinSet::new();
    let mut ipv6_interfaces = BTreeSet::new();
    for address in addresses {
        match address {
            InterfaceAddress::V4(address) if options.ipv4 => {
                run_ipv4_subnet(
                    &mut tasks,
                    address,
                    resolve,
                    ping_backend,
                    options.timeout,
                    semaphore.clone(),
                );
            }
            InterfaceAddress::V4(_) => {}
            InterfaceAddress::V6 {
                ip: _,
                interface,
                index,
            } if options.ipv6 => {
                ipv6_interfaces.insert((interface, index));
            }
            InterfaceAddress::V6 { .. } => {}
        }
    }

    if system_ping_exists {
        for (interface, index) in ipv6_interfaces {
            run_ipv6_interface(
                &mut tasks,
                &interface,
                index,
                resolve,
                options.timeout,
                semaphore.clone(),
            )
            .await;
        }
    }

    let mut seen = BTreeSet::new();
    while let Some(result) = tasks.join_next().await {
        if let Ok(Some(result)) = result
            && seen.insert(result.clone())
        {
            on_result(result);
        }
    }

    Ok(())
}

/// Ping all the IP addresses on the local IPv4 `/24`.
fn run_ipv4_subnet(
    tasks: &mut JoinSet<Option<String>>,
    address: std::net::Ipv4Addr,
    resolve_hostnames: bool,
    ping_backend: PingBackend,
    timeout: usize,
    semaphore: Arc<Semaphore>,
) {
    let octets = address.octets();

    for i in 1..255 {
        let ip_addr = IpAddr::V4(std::net::Ipv4Addr::new(octets[0], octets[1], octets[2], i));
        tasks.spawn(ping_address(
            ip_addr,
            resolve_hostnames,
            ping_backend,
            timeout,
            semaphore.clone(),
        ));
    }
}

async fn run_ipv6_interface(
    tasks: &mut JoinSet<Option<String>>,
    interface: &str,
    index: Option<u32>,
    resolve_hostnames: bool,
    timeout: usize,
    semaphore: Arc<Semaphore>,
) {
    let addresses = system_ipv6_multicast_ping(interface, index, timeout).await;

    for address in addresses {
        tasks.spawn(format_successful_address(
            address,
            resolve_hostnames,
            semaphore.clone(),
        ));
    }
}

async fn ping_address(
    ip_addr: IpAddr,
    resolve_hostnames: bool,
    ping_backend: PingBackend,
    timeout: usize,
    semaphore: Arc<Semaphore>,
) -> Option<String> {
    let _permit = match semaphore.acquire().await {
        Ok(permit) => permit,
        Err(_) => return None,
    };

    let success = match ping_backend {
        PingBackend::RawSocket => socket_ping(&ip_addr, timeout).await,
        PingBackend::System => system_ping(&ip_addr, timeout).await,
    };

    match (success, resolve_hostnames) {
        (true, true) => resolve_hostname(&ip_addr)
            .await
            .or_else(|| Some(ip_addr.to_string())),
        (true, false) => Some(ip_addr.to_string()),
        _ => None,
    }
}

async fn format_successful_address(
    address: DiscoveredAddress,
    resolve_hostnames: bool,
    semaphore: Arc<Semaphore>,
) -> Option<String> {
    let _permit = match semaphore.acquire().await {
        Ok(permit) => permit,
        Err(_) => return None,
    };

    if resolve_hostnames {
        resolve_hostname(&address.ip_addr)
            .await
            .or(Some(address.display_addr))
    } else {
        Some(address.display_addr)
    }
}

#[doc(hidden)]
pub mod cli_support {
    pub use super::util::{
        PingBackend, can_open_raw_socket, command_exists, hostname_resolution_supported,
        raw_socket_supported, select_ping_backend,
    };
}
