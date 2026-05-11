use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::process::Stdio;
#[cfg(unix)]
use std::time::Duration;

use clap::{ArgAction, Parser};
use if_addrs::{IfAddr, get_if_addrs};
use tokio::process::Command;

#[cfg(unix)]
use tiny_ping::Pinger;

#[derive(Debug, Parser)]
#[command(version, about)]
pub(crate) struct Args {
    /// Interface to search.
    #[arg(short, long)]
    pub(crate) interface: Option<String>,

    /// Don't attempt to resolve hostnames.
    #[arg(short = 'd', long = "dont-resolve", alias = "no-resolve", action = ArgAction::SetTrue)]
    pub(crate) dont_resolve: bool,

    /// Open raw socket instead of using system `ping` command. Unix only, requires permissions.
    #[arg(short = 'r', long = "raw-socket", action = ArgAction::SetTrue)]
    pub(crate) raw_socket: bool,

    /// Timeout of pings in seconds.
    #[arg(short, long, default_value_t = 1)]
    pub(crate) timeout: usize,

    /// Scan IPv4 addresses only.
    #[arg(short = '4', long = "ipv4", conflicts_with = "ipv6", action = ArgAction::SetTrue)]
    pub(crate) ipv4: bool,

    /// Scan IPv6 addresses only.
    #[arg(short = '6', long = "ipv6", conflicts_with = "ipv4", action = ArgAction::SetTrue)]
    pub(crate) ipv6: bool,
}

impl Args {
    pub(crate) fn scan_ipv4(&self) -> bool {
        !self.ipv6
    }

    pub(crate) fn scan_ipv6(&self) -> bool {
        !self.ipv4
    }
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
    Args::parse()
}

/// Check if a command is available in the current `$PATH`.
pub(crate) fn command_exists(command: &str) -> bool {
    which::which(command).is_ok()
}

pub(crate) fn hostname_resolution_supported() -> bool {
    if cfg!(target_os = "linux") {
        command_exists("avahi-resolve")
    } else {
        cfg!(windows)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum InterfaceAddress {
    V4(Ipv4Addr),
    V6 {
        ip: Ipv6Addr,
        interface: String,
        index: Option<u32>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoveredAddress {
    pub(crate) ip_addr: IpAddr,
    pub(crate) display_addr: String,
}

/// List the IP addresses associated with an interface.
/// Given no interface, list all non-loopback IP addresses of all interfaces.
pub(crate) fn get_addresses(interface: Option<String>) -> Vec<InterfaceAddress> {
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
            IfAddr::V4(addr) => Some(InterfaceAddress::V4(addr.ip)),
            IfAddr::V6(addr) => {
                if addr.ip.is_unspecified() || addr.ip.is_multicast() {
                    None
                } else {
                    Some(InterfaceAddress::V6 {
                        ip: addr.ip,
                        interface: ifaddr.name,
                        index: ifaddr.index,
                    })
                }
            }
        }
    });

    addresses.collect()
}

#[allow(dead_code)]
fn format_hostname(ip_addr: &IpAddr, hostname: &str) -> Option<String> {
    let hostname = hostname.trim().trim_end_matches('.');
    if hostname.is_empty() || hostname == ip_addr.to_string() {
        return None;
    }

    Some(format!("{}\t{}", ip_addr, hostname))
}

#[cfg(target_os = "linux")]
fn parse_avahi_resolve_output(ip_addr: &IpAddr, output: &[u8]) -> Option<String> {
    let output = String::from_utf8_lossy(output);

    output.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let ip = parts.next()?;
        let hostname = parts.next()?;

        if ip == ip_addr.to_string() {
            format_hostname(ip_addr, hostname)
        } else {
            None
        }
    })
}

#[cfg(target_os = "linux")]
pub(crate) async fn resolve_hostname(ip_addr: &IpAddr) -> Option<String> {
    let output = Command::new("avahi-resolve")
        .arg("--address")
        .arg(ip_addr.to_string())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;

    if output.status.success() && !output.stdout.is_empty() {
        parse_avahi_resolve_output(ip_addr, &output.stdout)
    } else {
        None
    }
}

#[cfg(windows)]
pub(crate) async fn resolve_hostname(ip_addr: &IpAddr) -> Option<String> {
    let ip_addr = *ip_addr;
    let lookup = tokio::task::spawn_blocking(move || dns_lookup::lookup_addr(&ip_addr))
        .await
        .ok()?
        .ok()?;

    format_hostname(&ip_addr, &lookup)
}

#[cfg(not(any(target_os = "linux", windows)))]
pub(crate) async fn resolve_hostname(_ip_addr: &IpAddr) -> Option<String> {
    None
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
        PingPlatform::Linux | PingPlatform::OtherUnix => {
            let mut args = Vec::new();
            if ip_addr.is_ipv6() {
                args.push("-6".to_string());
            }
            args.extend([
                "-c".to_string(),
                "1".to_string(),
                "-W".to_string(),
                timeout.to_string(),
                ip_addr.to_string(),
            ]);
            args
        }
        PingPlatform::Macos => vec![
            "-c".to_string(),
            "1".to_string(),
            "-W".to_string(),
            timeout.saturating_mul(1000).to_string(),
            ip_addr.to_string(),
        ],
    }
}

fn system_ping_command(platform: PingPlatform, ip_addr: &IpAddr) -> &'static str {
    if platform == PingPlatform::Macos && ip_addr.is_ipv6() {
        "ping6"
    } else {
        "ping"
    }
}

/// Ping using system `ping` command.
pub(crate) async fn system_ping(ip_addr: &IpAddr, timeout: usize) -> bool {
    let platform = current_ping_platform();
    let args = system_ping_args(platform, ip_addr, timeout);
    let mut command = match Command::new(system_ping_command(platform, ip_addr))
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

fn scoped_ipv6_multicast_target(
    platform: PingPlatform,
    interface: &str,
    index: Option<u32>,
) -> String {
    let scope = if platform == PingPlatform::Windows {
        index.map_or_else(|| interface.to_string(), |index| index.to_string())
    } else {
        interface.to_string()
    };

    format!("ff02::1%{}", scope)
}

fn system_ipv6_multicast_ping_command(platform: PingPlatform) -> &'static str {
    if platform == PingPlatform::Macos {
        "ping6"
    } else {
        "ping"
    }
}

fn system_ipv6_multicast_ping_args(
    platform: PingPlatform,
    interface: &str,
    index: Option<u32>,
    timeout: usize,
) -> Vec<String> {
    let target = scoped_ipv6_multicast_target(platform, interface, index);

    match platform {
        PingPlatform::Windows => vec![
            "/n".to_string(),
            "1".to_string(),
            "/w".to_string(),
            timeout.saturating_mul(1000).to_string(),
            target,
        ],
        PingPlatform::Linux | PingPlatform::OtherUnix => vec![
            "-6".to_string(),
            "-w".to_string(),
            timeout.to_string(),
            target,
        ],
        PingPlatform::Macos => vec![
            "-c".to_string(),
            "1".to_string(),
            "-W".to_string(),
            timeout.saturating_mul(1000).to_string(),
            target,
        ],
    }
}

fn parse_ping_reply_addresses(output: &[u8]) -> Vec<DiscoveredAddress> {
    let output = String::from_utf8_lossy(output);
    let mut seen = BTreeSet::new();

    output
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            while let Some(word) = words.next() {
                if word.eq_ignore_ascii_case("from") {
                    let mut candidate = words.next()?.trim_matches(|c| c == '[' || c == ']');

                    while let Some(stripped) = candidate
                        .strip_suffix(':')
                        .or_else(|| candidate.strip_suffix(','))
                    {
                        candidate = stripped;
                    }

                    let display_addr = candidate.to_string();
                    let ip_addr = candidate.split_once('%').map_or(candidate, |(ip, _)| ip);
                    return ip_addr
                        .parse::<IpAddr>()
                        .ok()
                        .map(|ip_addr| DiscoveredAddress {
                            ip_addr,
                            display_addr,
                        });
                }
            }

            None
        })
        .filter(|address| seen.insert(address.display_addr.clone()))
        .collect()
}

/// Ping the scoped IPv6 all-nodes multicast address on an interface and return responders.
pub(crate) async fn system_ipv6_multicast_ping(
    interface: &str,
    index: Option<u32>,
    timeout: usize,
) -> Vec<DiscoveredAddress> {
    let platform = current_ping_platform();
    let args = system_ipv6_multicast_ping_args(platform, interface, index, timeout);
    let output = match Command::new(system_ipv6_multicast_ping_command(platform))
        .args(args)
        .stderr(Stdio::null())
        .output()
        .await
    {
        Ok(output) => output,
        Err(_) => return Vec::new(),
    };

    parse_ping_reply_addresses(&output.stdout)
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
        PingBackend, PingPlatform, RuntimePlatform, format_hostname, parse_ping_reply_addresses,
        scoped_ipv6_multicast_target, select_ping_backend_for, system_ipv6_multicast_ping_args,
        system_ping_args,
    };
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

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
    fn linux_ipv6_ping_args_force_ipv6() {
        let ip = IpAddr::V6(Ipv6Addr::LOCALHOST);

        assert_eq!(
            system_ping_args(PingPlatform::Linux, &ip, 1),
            vec!["-6", "-c", "1", "-W", "1", "::1"]
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
    fn ipv6_multicast_target_uses_windows_interface_index_when_available() {
        assert_eq!(
            scoped_ipv6_multicast_target(PingPlatform::Windows, "Ethernet", Some(12)),
            "ff02::1%12"
        );
    }

    #[test]
    fn linux_ipv6_multicast_ping_args_use_scoped_all_nodes_address() {
        assert_eq!(
            system_ipv6_multicast_ping_args(PingPlatform::Linux, "eth0", Some(2), 1),
            vec!["-6", "-w", "1", "ff02::1%eth0"]
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

    #[test]
    fn hostname_output_matches_existing_ip_hostname_format() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));

        assert_eq!(
            format_hostname(&ip, "printer.local."),
            Some("192.168.1.10\tprinter.local".to_string())
        );
    }

    #[test]
    fn hostname_output_ignores_empty_and_numeric_names() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));

        assert_eq!(format_hostname(&ip, ""), None);
        assert_eq!(format_hostname(&ip, "192.168.1.10"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn avahi_output_parses_ip_hostname_line() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));

        assert_eq!(
            super::parse_avahi_resolve_output(&ip, b"192.168.1.10\tprinter.local\n"),
            Some("192.168.1.10\tprinter.local".to_string())
        );
    }

    #[test]
    fn ping_reply_parser_finds_ipv6_replies_with_scopes() {
        let replies = parse_ping_reply_addresses(
            b"64 bytes from fe80::5054:ff:fe12:3456%eth0: icmp_seq=1 ttl=64 time=0.1 ms\n",
        );

        assert_eq!(
            replies,
            vec![super::DiscoveredAddress {
                ip_addr: IpAddr::V6("fe80::5054:ff:fe12:3456".parse::<Ipv6Addr>().unwrap()),
                display_addr: "fe80::5054:ff:fe12:3456%eth0".to_string(),
            }]
        );
    }

    #[test]
    fn ping_reply_parser_finds_windows_ipv6_replies() {
        let replies = parse_ping_reply_addresses(b"Reply from fe80::1%12: time<1ms\n");

        assert_eq!(
            replies,
            vec![super::DiscoveredAddress {
                ip_addr: IpAddr::V6("fe80::1".parse().unwrap()),
                display_addr: "fe80::1%12".to_string(),
            }]
        );
    }
}
