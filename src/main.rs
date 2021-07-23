use std::net::IpAddr;
use std::process::Stdio;

use pingall::Opt;

use structopt::StructOpt;
use tokio::process::Command;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();
    println!("{:?}", opt.subnet);

    let mut pings = Vec::with_capacity(255);
    for i in 1..255 {
        let ip_addr = IpAddr::V4(format!("{}{}", opt.subnet, i).parse().unwrap());
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
                let get_hostname = Command::new("avahi-resolve-address")
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
