use std::collections::BTreeSet;
use std::io::{IsTerminal, stderr, stdout};
use std::net::IpAddr;
use std::process::Stdio;
use std::sync::Arc;

mod util;
use util::{
    PingBackend, can_open_raw_socket, command_exists, get_addresses, get_args,
    raw_socket_supported, select_ping_backend, socket_ping, system_ping,
};

use tokio::process::Command;
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
    } else if cfg!(target_os = "linux") {
        match command_exists("avahi-resolve") {
            true => true,
            false => {
                if stdout().is_terminal() {
                    eprintln!("`avahi-resolve` not found, hostname resolution disabled");
                }
                false
            }
        }
    } else {
        false
    };

    if args.raw_socket && !raw_socket_supported() && stderr().is_terminal() {
        eprintln!(
            "Raw socket mode is unsupported on this platform; falling back to system `ping`."
        );
    }

    // Whether to use system `ping` command, or open a socket ourselves.
    let ping_backend = select_ping_backend(args.raw_socket, command_exists("ping"))?;

    // Check we have permission to open raw sockets.
    if ping_backend == PingBackend::RawSocket
        && !can_open_raw_socket().await
        && stderr().is_terminal()
    {
        let err_msg = "Either run as root, or run `setcap cap_net_raw+ep $(which pingall)` to allow this app to open raw sockets.";
        eprintln!("Error opening raw socket.\n{}", err_msg);
    }

    // Get our IP address on each interface we'll be checking.
    let addresses = get_addresses(args.interface);

    // Create a semaphore to limit concurrent operations (prevents "too many open files")
    let semaphore = Arc::new(Semaphore::new(150)); // Allow max 50 concurrent pings

    // Ping the subnet and record replies.
    let mut results = vec![];
    for address in addresses {
        let octets = address.octets();
        let ip_subnet = format!("{}.{}.{}.", octets[0], octets[1], octets[2]);
        results.extend(
            run_subnet(
                &ip_subnet,
                resolve,
                ping_backend,
                args.timeout,
                semaphore.clone(),
            )
            .await?,
        );
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

/// Ping all the IP addresses on a subnet formatted `X.X.X.`.
async fn run_subnet(
    subnet: &str,
    resolve_hostname: bool,
    ping_backend: PingBackend,
    timeout: usize,
    semaphore: Arc<Semaphore>,
) -> Result<PingResults, Box<dyn std::error::Error>> {
    Ok((1..255)
        .filter_map(|i| {
            // Parse IP address safely
            let ip_str = format!("{}{}", subnet, i);
            match ip_str.parse() {
                Ok(ip_v4) => {
                    let ip_addr = IpAddr::V4(ip_v4);
                    let semaphore = semaphore.clone();
                    Some(tokio::spawn(async move {
                        // Acquire permit before doing any work
                        let _permit = match semaphore.acquire().await {
                            Ok(permit) => permit,
                            Err(_) => return None, // Semaphore closed, skip this ping
                        };

                        // Ping the address.
                        let success = match ping_backend {
                            PingBackend::RawSocket => socket_ping(&ip_addr, timeout).await,
                            PingBackend::System => system_ping(&ip_addr, timeout).await,
                        };

                        match (success, resolve_hostname) {
                            (true, true) => {
                                // Try to resolve hostname with `avahi-resolve`.
                                let get_hostname = Command::new("avahi-resolve")
                                    .arg("--address")
                                    .arg(ip_addr.to_string())
                                    .stderr(Stdio::null())
                                    .output();

                                match get_hostname.await {
                                    Ok(output) => {
                                        if output.status.success() && !output.stdout.is_empty() {
                                            // Send back hostname and IP.
                                            let utf8_out = String::from_utf8_lossy(&output.stdout)
                                                .trim()
                                                .to_string();
                                            Some(utf8_out)
                                        } else {
                                            // Only send back the IP addr.
                                            Some(ip_addr.to_string())
                                        }
                                    }
                                    Err(_) => {
                                        // If avahi-resolve fails, just return the IP
                                        Some(ip_addr.to_string())
                                    }
                                }
                            }
                            (true, false) => Some(ip_addr.to_string()),
                            _ => None,
                        }
                    }))
                }
                Err(_) => {
                    // Skip invalid IP addresses silently
                    None
                }
            }
        })
        .collect())
}
