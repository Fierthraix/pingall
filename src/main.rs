use std::collections::BTreeSet;
use std::io::{IsTerminal, stderr, stdout};
use std::net::IpAddr;
use std::process::Stdio;

mod util;
use util::{
    can_open_raw_socket, command_exists, get_addresses, get_args, socket_ping, system_ping,
};

use tokio::process::Command;

type PingResults = Vec<tokio::task::JoinHandle<Option<String>>>;

fn main() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            run().await.unwrap();
        })
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Parse CLI args.
    let args = get_args();

    // Whether to attempt to resolve hostnames.
    let resolve = match (args.dont_resolve, command_exists("avahi-resolve")) {
        (false, true) => true,
        (false, false) => {
            if stdout().is_terminal() {
                eprintln!("`avahi-resolve` not found, hostname resolution disabled");
            }
            false
        }
        _ => false,
    };

    // Whether to use system `ping` command, or open a socket ourselves.
    let open_raw_socket = args.raw_socket || !command_exists("ping");

    // Check we have permission to open raw sockets.
    if open_raw_socket && !can_open_raw_socket().await && stderr().is_terminal() {
        let err_msg = "Either run as root, or run `setcap cap_net_raw+ep $(which pingall)` to allow this app to open raw sockets.";
        eprintln!("Error opening raw socket.\n{}", err_msg);
    }

    // Get our IP address on each interface we'll be checking.
    let addresses = get_addresses(args.interface);

    // Ping the subnet and record replies.
    let mut results = vec![];
    for address in addresses {
        let octets = address.octets();
        let ip_subnet = format!("{}.{}.{}.", octets[0], octets[1], octets[2]);
        results.extend(run_subnet(&ip_subnet, resolve, open_raw_socket, args.timeout).await?);
    }

    let mut seen = BTreeSet::new();
    // Print successful pings.
    for ping in results {
        if let Some(result) = ping.await? {
            if !seen.contains(&result) {
                println!("{}", result);
                seen.insert(result);
            }
        }
    }

    Ok(())
}

/// Ping all the IP addresses on a subnet formatted `X.X.X.`.
async fn run_subnet(
    subnet: &str,
    resolve_hostname: bool,
    open_socket: bool,
    timeout: usize,
) -> Result<PingResults, Box<dyn std::error::Error>> {
    Ok((1..255)
        .map(|i| {
            let ip_addr = IpAddr::V4(format!("{}{}", subnet, i).parse().unwrap());
            tokio::spawn(async move {
                // Ping the address.
                let success = if open_socket {
                    socket_ping(&ip_addr, timeout).await
                } else {
                    system_ping(&ip_addr, timeout).await
                };

                match (success, resolve_hostname) {
                    (true, true) => {
                        // Try to resolve hostname with `avahi-resolve`.
                        let get_hostname = Command::new("avahi-resolve")
                            .arg("--address")
                            .arg(ip_addr.to_string())
                            .stderr(Stdio::null())
                            .output();

                        let output = get_hostname.await.unwrap();
                        if output.status.success() && !output.stdout.is_empty() {
                            // Send back hostname and IP.
                            let utf8_out =
                                String::from_utf8_lossy(&output.stdout).trim().to_string();
                            Some(utf8_out)
                        } else {
                            // Only send back the IP addr.
                            Some(ip_addr.to_string())
                        }
                    }
                    (true, false) => Some(ip_addr.to_string()),
                    _ => None,
                }
            })
        })
        .collect())
}
