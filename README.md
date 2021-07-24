# pingall
Utility to _quickly_ and _efficiently_ discover available ip addresses and their hostnames on your network.


## Installation

### Dependencies
To resolve hostnames, [avahi-resolve](https://command-not-found.com/avahi-resolve) is needed.

```bash
cargo install pingall
```

## Usage
Ping all available ip addresses:
```bash
pingall
```

Ping only Wi-Fi addresses on `wlan0`:
```bash
pingall -i wlan0
```
