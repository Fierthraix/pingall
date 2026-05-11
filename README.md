# pingall
Utility to _quickly_ and _efficiently_ discover available IP addresses and their hostnames on your network. This program will always finish within a small timeout, much faster than `nmap 192.168.1.*`.

* Quickly find IPv4 and IPv6 addresses and hostnames on your network.
* Populate DNS/mDNS tables automatically.

## Usage

```bash
USAGE:
    pingall [OPTIONS]

OPTIONS:
    -i, --interface <INTERFACE>    Interface to search
    -d, --dont-resolve            Don't attempt to resolve hostnames
    -r, --raw-socket              Open raw socket instead of using system `ping` command. Unix only, requires permissions
    -t, --timeout <TIMEOUT>        Timeout of pings in seconds [default: 1]
    -4, --ipv4                    Scan IPv4 addresses only
    -6, --ipv6                    Scan IPv6 addresses only
    -h, --help                    Print help
    -V, --version                 Print version
```

Ping all available ip addresses:
```bash
$ pingall
192.168.0.1        router.local
192.168.0.19       SAMSUNG-GALAXY-8
192.168.0.98       raspberrypi.local
10.10.0.132
10.10.0.152        vps.local
10.10.0.243
fe80::5054:ff:fe12:3456%wlan0
```

Ping only Wi-Fi addresses on `wlan0`, don't resolve hostnames:
```bash
pingall --interface wlan0 --dont-resolve
192.168.0.1
192.168.0.19
192.168.0.98
```

Scan only one address family:
```bash
pingall --ipv4
pingall --ipv6 --interface wlan0
```

## Installation
```bash
cargo install pingall
```

On Arch Linux, `pingall` is also available from the AUR as
[`pingall`](https://aur.archlinux.org/packages/pingall) for the latest crates.io
release or [`pingall-git`](https://aur.archlinux.org/packages/pingall-git) for
the current Git version.

## Details
Simultaneously `ping` all of the IPv4 addresses on your local `/24` subnets with a 1 second timeout, so we can gauge who is responsive on the network. IPv6 discovery uses the scoped all-nodes multicast address (`ff02::1%interface`) because typical IPv6 subnets are too large to sweep. [tokio](https://tokio.rs/) is used to make it all asynchronous (only 1 thread is used).

### Raw Ping
The system `ping` command is used by default. On Windows, `pingall` always uses the system `ping` command. On Unix systems, opening raw sockets requires elevated permissions. To avoid using the ping command for IPv4 sweeps, you can use the `--raw-socket` flag, but this will require either `sudo`, or running
```
setcap cap_net_raw+ep $(which pingall)
```
to give this program permission. IPv6 multicast discovery uses the system `ping` command even when raw socket mode is requested.

### Dependencies
* [cargo](https://rustup.rs/)
* [ping](https://command-not-found.com/ping)
* [avahi-resolve](https://command-not-found.com/avahi-resolve) on Linux (needed to resolve hostnames)

Hostname resolution uses `avahi-resolve` on Linux and the operating system reverse lookup APIs on Windows.
