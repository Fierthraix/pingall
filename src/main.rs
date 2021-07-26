use std::net::IpAddr;
use std::process::Stdio;

mod util;
use util::{command_exists, get_addresses, socket_ping, system_ping, Opt};

use structopt::StructOpt;
use tokio::process::Command;

type PingResults = Vec<tokio::task::JoinHandle<Option<String>>>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse CLI args.
    let opt = Opt::from_args();

    // Whether to attempt to resolve hostnames.
    let resolve = !opt.dont_resolve && command_exists("avahi-resolve");

    // Whether to use system `ping` command, or open socket.
    let open_raw_socket = opt.raw_socket && command_exists("ping");

    // Get our IP address on each interface we'll be checking.
    let addresses = get_addresses(opt.interface);

    // Ping the subnet and record replies.
    let mut results = vec![];
    for address in addresses {
        let octets = address.octets();
        let ip_subnet = format!("{}.{}.{}.", octets[0], octets[1], octets[2]);
        results.extend(run_subnet(&ip_subnet, resolve, open_raw_socket).await?);
    }

    // Print successful pings.
    for ping in results {
        if let Some(result) = ping.await? {
            println!("{}", result);
        }
    }

    Ok(())
}

async fn run_subnet(
    subnet: &str,
    resolve: bool,
    open_socket: bool,
) -> Result<PingResults, Box<dyn std::error::Error>> {
    Ok((1..255)
        .map(|i| {
            let ip_addr = IpAddr::V4(format!("{}{}", subnet, i).parse().unwrap());
            // Ping the address.
            tokio::spawn(async move {
                let success = if open_socket {
                    socket_ping(&ip_addr).await
                } else {
                    system_ping(&ip_addr).await
                };

                match (success, resolve) {
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
