use std::collections::BTreeSet;
use std::io::{IsTerminal, stderr, stdout};
use std::net::IpAddr;
use std::sync::Arc;

mod util;
use util::{
    DiscoveredAddress, InterfaceAddress, PingBackend, can_open_raw_socket, command_exists,
    get_addresses, get_args, hostname_resolution_supported, raw_socket_supported, resolve_hostname,
    select_ping_backend, socket_ping, system_ipv6_multicast_ping, system_ping,
};

use tokio::sync::Semaphore;

type PingResults = Vec<tokio::task::JoinHandle<Option<String>>>;

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
    // Parse CLI args.
    let args = get_args();

    // Whether to attempt to resolve hostnames.
    let resolve = if args.dont_resolve {
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

    // Whether to use system `ping` command, or open a socket ourselves.
    let system_ping_exists = command_exists("ping");
    if args.scan_ipv6() && !system_ping_exists {
        return Err("system `ping` command not found and IPv6 discovery requires it".into());
    }

    let ping_backend = select_ping_backend(args.raw_socket, system_ping_exists)?;

    // Check we have permission to open raw sockets.
    if args.scan_ipv4()
        && ping_backend == PingBackend::RawSocket
        && !can_open_raw_socket().await
        && stderr().is_terminal()
    {
        let err_msg = "Either run as root, or run `setcap cap_net_raw+ep $(which pingall)` to allow this app to open raw sockets.";
        eprintln!("Error opening raw socket.\n{}", err_msg);
    }

    // Get our IP address on each interface we'll be checking.
    let addresses = get_addresses(args.interface.clone());

    // Create a semaphore to limit concurrent operations (prevents "too many open files")
    let semaphore = Arc::new(Semaphore::new(150));

    // Ping the subnet and record replies.
    let mut results = vec![];
    let mut ipv6_interfaces = BTreeSet::new();
    for address in addresses {
        match address {
            InterfaceAddress::V4(address) if args.scan_ipv4() => {
                results.extend(
                    run_ipv4_subnet(
                        address,
                        resolve,
                        ping_backend,
                        args.timeout,
                        semaphore.clone(),
                    )
                    .await?,
                );
            }
            InterfaceAddress::V4(_) => {}
            InterfaceAddress::V6 {
                ip: _,
                interface,
                index,
            } if args.scan_ipv6() => {
                ipv6_interfaces.insert((interface, index));
            }
            InterfaceAddress::V6 { .. } => {}
        }
    }

    if system_ping_exists {
        for (interface, index) in ipv6_interfaces {
            results.extend(
                run_ipv6_interface(&interface, index, resolve, args.timeout, semaphore.clone())
                    .await?,
            );
        }
    }

    let mut seen = BTreeSet::new();
    // Print successful pings.
    for ping in results {
        match ping.await {
            Ok(Some(result)) => {
                if !seen.contains(&result) {
                    println!("{}", result);
                    seen.insert(result);
                }
            }
            Ok(None) => {
                // Ping failed, continue to next
            }
            Err(e) => {
                // Task panicked or was cancelled, log but continue
                if stderr().is_terminal() {
                    eprintln!("Warning: ping task failed: {}", e);
                }
            }
        }
    }

    Ok(())
}

/// Ping all the IP addresses on the local IPv4 `/24`.
async fn run_ipv4_subnet(
    address: std::net::Ipv4Addr,
    resolve_hostnames: bool,
    ping_backend: PingBackend,
    timeout: usize,
    semaphore: Arc<Semaphore>,
) -> Result<PingResults, Box<dyn std::error::Error>> {
    let octets = address.octets();

    Ok((1..255)
        .map(|i| {
            let ip_addr = IpAddr::V4(std::net::Ipv4Addr::new(octets[0], octets[1], octets[2], i));
            ping_address(
                ip_addr,
                resolve_hostnames,
                ping_backend,
                timeout,
                semaphore.clone(),
            )
        })
        .collect())
}

async fn run_ipv6_interface(
    interface: &str,
    index: Option<u32>,
    resolve_hostnames: bool,
    timeout: usize,
    semaphore: Arc<Semaphore>,
) -> Result<PingResults, Box<dyn std::error::Error>> {
    let addresses = system_ipv6_multicast_ping(interface, index, timeout).await;

    Ok(addresses
        .into_iter()
        .map(|address| format_successful_address(address, resolve_hostnames, semaphore.clone()))
        .collect())
}

fn ping_address(
    ip_addr: IpAddr,
    resolve_hostnames: bool,
    ping_backend: PingBackend,
    timeout: usize,
    semaphore: Arc<Semaphore>,
) -> tokio::task::JoinHandle<Option<String>> {
    tokio::spawn(async move {
        // Acquire permit before doing any work.
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
    })
}

fn format_successful_address(
    address: DiscoveredAddress,
    resolve_hostnames: bool,
    semaphore: Arc<Semaphore>,
) -> tokio::task::JoinHandle<Option<String>> {
    tokio::spawn(async move {
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
    })
}
