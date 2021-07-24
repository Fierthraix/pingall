use std::net::IpAddr;
use std::process::Stdio;

use pingall::{command_exists, get_addresses, Opt};

use structopt::StructOpt;
use tokio::process::Command;

type PingResults = Vec<tokio::task::JoinHandle<Option<String>>>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();

    let resolve = !opt.dont_resolve && command_exists("avahi-resolve");

    let addresses = get_addresses(opt.interface);

    let mut results = vec![];
    for address in addresses {
        let octets = address.octets();
        let ip_subnet = format!("{}.{}.{}.", octets[0], octets[1], octets[2]);
        results.extend(run_subnet(&ip_subnet, resolve).await?);
    }

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
) -> Result<PingResults, Box<dyn std::error::Error>> {
    Ok((1..255)
        .map(|i| {
            let ip_addr = IpAddr::V4(format!("{}{}", subnet, i).parse().unwrap());
            // Ping the address.
            tokio::spawn(async move {
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
                let success = match command.wait().await {
                    Ok(status) => status.code().unwrap_or(1) == 0,
                    Err(_) => false,
                };

                if success {
                    if resolve {
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
                    } else {
                        // Only send back the IP addr.
                        Some(ip_addr.to_string())
                    }
                } else {
                    None
                }
            })
        })
        .collect())
}
