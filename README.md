# UDP Relay Service

## Overview

This Rust application provides a UDP relay service designed to facilitate the use of Mosh (mobile shell) where the Mosh server is behind an SSH jump proxy. The service acts as a relay for UDP packets, allowing connections to be established and maintained across a network boundary.
The service operates on a single UDP port and relays packets between peers based on unique pairing session secrets.

## Features

- **UDP Relay:** Relays UDP packets between peers.
- **Authenticate Mechanism:** Uses a pre-shared key to authenticate peers.
- **Pairing Mechanism:** Uses a session secret to pair peers (same session secret will be paired together).
- **Timeouts:** Configurable timeouts for socket wait, connection inactivity, and pairing.
- **Daemon Mode:** Optionally run as a daemon process.

## Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/) (version 1.50 or later)
- [Cargo](https://doc.rust-lang.org/cargo/) (comes with Rust)

### Installation

1. **Clone the Repository**

    ```bash
    git clone https://github.com/yourusername/udp-relay-service.git
    cd udp-relay-service
    ```

2. **Build the Project**

    ```bash
    cargo build --release
    ```

3. **Run the Application**

    ```bash
    cargo run --release -- --help
    ```

### Configuration

The application is configured via command-line arguments. Here are the available options:

- Argument `<port>`
  **UDP Port** for peer connections.

- `--bind-ip <ip>`
  **IP Address** to bind the UDP socket to. Default is `0.0.0.0`.

- `--verbose`
  Enable **verbose output** for debugging.

- `--daemonize`
  Run the service as a **daemon**.

- `--timeout-socket-wait <seconds>`
  Number of seconds before timing out the socket wait.

- `--timeout-no-connections <seconds>`
  Number of seconds before timing out with no connections.

- `--timeout-pairing <seconds>`
  Number of seconds before timing out the peer pairing.

- `--timeout-connection-inactivities <seconds>`
  Number of seconds before timing out connections with no activities.

- `--preshared-key <key>`
  Pre-shared key used for authentication. Default is `uNYDA5QRcvYgp2gfS5v5` which is just a randomly generated string.
  This can be changed to deny serving clients of using this relay service; however, since pairing is done via a session secret, exposing this PSK is not much of a security risk.


