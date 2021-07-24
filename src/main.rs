use std::net::IpAddr;
use std::process::Stdio;

use pingall::{command_exists, get_addresses, Opt};

use structopt::StructOpt;
use tokio::process::Command;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();

    let resolve = !opt.dont_resolve && command_exists("avahi-resolve");

    let addresses = get_addresses(opt.interface);

    for address in addresses {
        let octets = address.octets();
        run_subnet(
            &format!("{}.{}.{}.", octets[0], octets[1], octets[2]),
            resolve,
        )
        .await?;
    }

    Ok(())
}

async fn run_subnet(subnet: &str, resolve: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut pings = Vec::with_capacity(255);
    for i in 1..255 {
        let ip_addr = IpAddr::V4(format!("{}{}", subnet, i).parse().unwrap());
        pings.push(tokio::spawn(async move {
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

            let success = match command.wait().await {
                Ok(status) => status.code().unwrap_or(1) == 0,
                Err(_) => false,
            };

            if success {
                if resolve {
                    let get_hostname = Command::new("avahi-resolve")
                        .arg("--address")
                        .arg(ip_addr.to_string())
                        .stderr(Stdio::null())
                        .output();

                    let output = get_hostname.await.unwrap();
                    if output.status.success() && !output.stdout.is_empty() {
                        let utf8_out = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        Some(utf8_out)
                    } else {
                        Some(ip_addr.to_string())
                    }
                } else {
                    Some(ip_addr.to_string())
                }
            } else {
                None
            }
        }));
    }

    for ping in pings {
        if let Some(result) = ping.await? {
            println!("{}", result)
        }
    }

    Ok(())
}
