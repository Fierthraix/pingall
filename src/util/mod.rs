use std::net::{IpAddr, Ipv4Addr};
use std::process::Stdio;
#[cfg(unix)]
use std::time::Duration;

use if_addrs::{IfAddr, get_if_addrs};
use tokio::process::Command;

#[cfg(unix)]
use tiny_ping::Pinger;

const HELP: &str = r#"
USAGE:
    pingall [FLAGS]

FLAGS:
    -i <interface>        Interface to search
    -d, --dont-resolve    Don't attempt to resolve hostnames
    -h, --help            Prints help information
    -r, --raw-socket      Open raw socket instead of using system `ping` command. Unix only, requires permissions
    -t, --timeout         Timeout of pings in seconds (default 1)
    "#;

#[derive(Debug)]
pub(crate) struct Args {
    pub(crate) interface: Option<String>,

    pub(crate) dont_resolve: bool,

    pub(crate) raw_socket: bool,

    pub(crate) timeout: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PingBackend {
    System,
    RawSocket,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum RuntimePlatform {
    Unix,
    NonUnix,
}

#[cfg(unix)]
fn current_runtime_platform() -> RuntimePlatform {
    RuntimePlatform::Unix
}

#[cfg(not(unix))]
fn current_runtime_platform() -> RuntimePlatform {
    RuntimePlatform::NonUnix
}

pub(crate) fn raw_socket_supported() -> bool {
    current_runtime_platform() == RuntimePlatform::Unix
}

fn select_ping_backend_for(
    platform: RuntimePlatform,
    raw_socket_requested: bool,
    system_ping_exists: bool,
) -> Result<PingBackend, &'static str> {
    match platform {
        RuntimePlatform::Unix => {
            if raw_socket_requested || !system_ping_exists {
                Ok(PingBackend::RawSocket)
            } else {
                Ok(PingBackend::System)
            }
        }
        RuntimePlatform::NonUnix => {
            if system_ping_exists {
                Ok(PingBackend::System)
            } else {
                Err(
                    "system `ping` command not found and raw sockets are unsupported on this platform",
                )
            }
        }
    }
}

pub(crate) fn select_ping_backend(
    raw_socket_requested: bool,
    system_ping_exists: bool,
) -> Result<PingBackend, &'static str> {
    select_ping_backend_for(
        current_runtime_platform(),
        raw_socket_requested,
        system_ping_exists,
    )
}

pub(crate) fn get_args() -> Args {
    let mut pargs = pico_args::Arguments::from_env();

    if pargs.contains(["-h", "--help"]) {
        print!("{}", HELP);
        std::process::exit(0);
    }

    Args {
        interface: pargs.opt_value_from_str("-i").unwrap(),
        dont_resolve: pargs.contains(["-d", "--dont-resolve"]),
        raw_socket: pargs.contains(["-r", "--raw-socket"]),
        timeout: pargs
            .value_from_fn(["-t", "--timeout"], str::parse)
            .unwrap_or(1),
    }
}

/// Check if a command is available in the current `$PATH`.
pub(crate) fn command_exists(command: &str) -> bool {
    which::which(command).is_ok()
}

/// List the IPv4 address associated with an interface.
/// Given no interface, list all non-loopback IP addresses of all interfaces.
pub(crate) fn get_addresses(interface: Option<String>) -> Vec<Ipv4Addr> {
    let ifaddrs = match get_if_addrs() {
        Ok(ifaddrs) => ifaddrs,
        Err(_) => {
            eprintln!("Failed to get network interfaces");
            return Vec::new();
        }
    };

    let addresses = ifaddrs.into_iter().filter_map(|ifaddr| {
        if interface.as_ref().is_some_and(|name| ifaddr.name != *name) {
            return None;
        }

        if ifaddr.is_loopback() {
            return None;
        }

        match ifaddr.addr {
            IfAddr::V4(addr) => Some(addr.ip),
            IfAddr::V6(_) => None,
        }
    });

    if interface.is_some() {
        addresses.take(1).collect()
    } else {
        addresses.collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum PingPlatform {
    Windows,
    Linux,
    Macos,
    OtherUnix,
}

#[cfg(windows)]
fn current_ping_platform() -> PingPlatform {
    PingPlatform::Windows
}

#[cfg(target_os = "linux")]
fn current_ping_platform() -> PingPlatform {
    PingPlatform::Linux
}

#[cfg(target_os = "macos")]
fn current_ping_platform() -> PingPlatform {
    PingPlatform::Macos
}

#[cfg(all(not(windows), not(target_os = "linux"), not(target_os = "macos")))]
fn current_ping_platform() -> PingPlatform {
    PingPlatform::OtherUnix
}

fn system_ping_args(platform: PingPlatform, ip_addr: &IpAddr, timeout: usize) -> Vec<String> {
    match platform {
        PingPlatform::Windows => vec![
            "/n".to_string(),
            "1".to_string(),
            "/w".to_string(),
            timeout.saturating_mul(1000).to_string(),
            ip_addr.to_string(),
        ],
        PingPlatform::Linux | PingPlatform::OtherUnix => vec![
            "-c".to_string(),
            "1".to_string(),
            "-W".to_string(),
            timeout.to_string(),
            ip_addr.to_string(),
        ],
        PingPlatform::Macos => vec![
            "-c".to_string(),
            "1".to_string(),
            "-W".to_string(),
            timeout.saturating_mul(1000).to_string(),
            ip_addr.to_string(),
        ],
    }
}

/// Ping using system `ping` command.
pub(crate) async fn system_ping(ip_addr: &IpAddr, timeout: usize) -> bool {
    let args = system_ping_args(current_ping_platform(), ip_addr, timeout);
    let mut command = match Command::new("ping")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(cmd) => cmd,
        Err(_) => return false, // If we can't spawn ping, consider it failed
    };

    // Check if the ping succeeded.
    match command.wait().await {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

#[cfg(unix)]
pub(crate) async fn socket_ping(ip_addr: &IpAddr, timeout: usize) -> bool {
    if let Ok(mut pinger) = Pinger::new(*ip_addr) {
        pinger.timeout(Duration::from_secs(timeout as u64));
        return pinger.ping(0).await.is_ok();
    }
    false
}

#[cfg(not(unix))]
pub(crate) async fn socket_ping(_ip_addr: &IpAddr, _timeout: usize) -> bool {
    false
}

#[cfg(unix)]
pub(crate) async fn can_open_raw_socket() -> bool {
    let localhost = IpAddr::V4(Ipv4Addr::LOCALHOST);
    if let Ok(mut pinger) = Pinger::new(localhost) {
        pinger.timeout(Duration::from_secs(1));
        return pinger.ping(0).await.is_ok();
    }
    false
}

#[cfg(not(unix))]
pub(crate) async fn can_open_raw_socket() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{
        PingBackend, PingPlatform, RuntimePlatform, select_ping_backend_for, system_ping_args,
    };
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn windows_ping_args_use_count_and_millisecond_timeout() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        assert_eq!(
            system_ping_args(PingPlatform::Windows, &ip, 1),
            vec!["/n", "1", "/w", "1000", "192.168.1.1"]
        );
    }

    #[test]
    fn linux_ping_args_use_count_and_second_timeout() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        assert_eq!(
            system_ping_args(PingPlatform::Linux, &ip, 1),
            vec!["-c", "1", "-W", "1", "192.168.1.1"]
        );
    }

    #[test]
    fn macos_ping_args_use_count_and_millisecond_timeout() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        assert_eq!(
            system_ping_args(PingPlatform::Macos, &ip, 1),
            vec!["-c", "1", "-W", "1000", "192.168.1.1"]
        );
    }

    #[test]
    fn ping_backend_variants_stay_distinct() {
        assert_ne!(PingBackend::System, PingBackend::RawSocket);
    }

    #[test]
    fn unix_backend_uses_raw_socket_when_requested_or_ping_missing() {
        assert_eq!(
            select_ping_backend_for(RuntimePlatform::Unix, true, true),
            Ok(PingBackend::RawSocket)
        );
        assert_eq!(
            select_ping_backend_for(RuntimePlatform::Unix, false, false),
            Ok(PingBackend::RawSocket)
        );
        assert_eq!(
            select_ping_backend_for(RuntimePlatform::Unix, false, true),
            Ok(PingBackend::System)
        );
    }

    #[test]
    fn non_unix_backend_uses_system_ping_even_when_raw_requested() {
        assert_eq!(
            select_ping_backend_for(RuntimePlatform::NonUnix, true, true),
            Ok(PingBackend::System)
        );
    }

    #[test]
    fn non_unix_backend_errors_when_system_ping_is_missing() {
        assert!(select_ping_backend_for(RuntimePlatform::NonUnix, false, false).is_err());
    }
}
