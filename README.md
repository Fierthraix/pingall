# pingall
Utility to _quickly_ and _efficiently_ discover available ip addresses and their hostnames on your network. This program will always finish within 2 seconds, much faster than `nmap 196.168.1.*`.

### Details
By simultaneously `ping`ing all of the addresses with a 1 second timeout we can guage who is responsive on the network. [tokio](https://tokio.rs/) is used to make it all asynchronous (only 1 thread is used).

## Installation
```bash
cargo install pingall
```

### Dependencies
* [cargo](https://rustup.rs/)
* [ping](https://command-not-found.com/ping)
* [avahi-resolve](https://command-not-found.com/avahi-resolve) (needed to resolve hostnames)


## Usage

```bash
pingall

USAGE:
    pingall [FLAGS] [OPTIONS]

FLAGS:
    -d, --dont-resolve    Don't attempt to resolve hostnames
    -h, --help            Prints help information
    -V, --version         Prints version information

OPTIONS:
    -i, --interface <interface>    Interface to search
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
```

Ping only Wi-Fi addresses on `wlan0`, don't resolve hostnames:
```bash
pingall -i wlan0 --dont-resolve
192.168.0.1
192.168.0.19
192.168.0.98
```
