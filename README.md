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


### Example Usage

To run the service with default settings:

```bash
cargo run --release -- 12345
```

## How It Works

```sh
PeerA  ---------  X  ---------> PeerB
PeerA <---------  X  ---------  PeerB
# not reachable in either direction
PeerA  -----------------------> Relay
PeerA  -----------------------> Relay
# but both peers can realy Relay server

# so we establish UDP hole-punching
PeerA <-------> Relay <-------> PeerB
# using a relay server to relay the UDP packets
```

1. **Multiplexing on Single Port:** The service listens on a single UDP port and uses pairing session secrets to manage multiple peer connections. All relayed messages are handled over this single port.

2. **Pairing Request:** When a peer wants to establish a connection, it should send a pairing request message with a pre-shared key and a session secret, formatted as shown in the following section.

3. **Authentication and Pairing:** The service authenticates the request using the pre-shared key. If valid, it sets up a pair using the session secret to uniquely identify and manage the connection.

4. **Message Relaying:** Once a pair is established, the service relays UDP packets between the paired peers using the session secret to route messages correctly.

5. **Timeouts:** The service handles timeouts for idle connections and pairing requests to ensure efficient operation and resource management. When there are no activities, the service will eventually exit by itself.


## Pairing Request Message Format

The pairing request message is structured as follows:

1. **Command Bytes (2 bytes)**
   The first two bytes indicate the command type of the request. For example, `[0xff, 0x05]` denotes the operation for establishing a connection.

2. **Pre-Shared Key (PSK) Length (1 byte)**
   The third byte specifies the length of the Pre-Shared Key (PSK) encoded in the message. This length is denoted as `P`, where `P` is an unsigned 8-bit integer (`u8`).

3. **Session Secret Length (1 byte)**
   The fourth byte specifies the length of the session secret encoded in the message. This length is denoted as `S`, where `S` is an unsigned 8-bit integer (`u8`).

4. **Pre-Shared Key (PSK) (P bytes)**
   Following the first four bytes, there are `P` bytes which represent the Pre-Shared Key.

5. **Session Secret (S bytes)**
   After the PSK, there are `S` bytes which represent the session secret.

### Message Validation
- The total length of the message must be at least `4 + P + S` bytes.
- If the message length is shorter than this, the message is discarded.

Here is an ASCII diagram that illustrates the format:

```
+-----------+---------+---------+----------+-----------+
| Command   |  PSK    | Secret  |   PSK    |  Secret   |
|           | Length  | Length  |          |           |
| (2 bytes) | (1 byte)| (1 byte)| (P byte) | (S bytes) |
+-----------+---------+---------+----------+-----------+
| 0xff 0x05 | ....... | ....... | ........ | ......... |
|           |    P    |    S    |          |           |
+-----------+---------+---------+----------+-----------+
```

- **Command**: Identifies the operation type.
- **PSK Length**: Indicates the length of the Pre-Shared Key.
- **Session Secret Length**: Indicates the length of the session secret.
- **PSK**: The Pre-Shared Key used for authentication.
- **Session Secret**: The unique secret used for identifying and managing the peer connection.

### Important Notes

- Messages shorter than `4 + P + S` bytes are invalid and will be dropped.
- Ensure the PSK and Session Secret lengths are correctly specified and matched in the message.

### Example Message

For example, consider a message with:
- Command: `[0xff, 0x05]` (2 bytes)
- PSK length: `3` (1 byte)
- Session Secret length: `5` (1 byte)
- PSK: `abc` (3 bytes)
- Session Secret: `12345` (5 bytes)

The message would look like this in bytes:

```
[0xff, 0x05, 3, 5, a, b, c, 1, 2, 3, 4, 5]
```

Here’s a visual representation:

```
+------+------+------+------+------------------+------------------------------------+
| 0xff | 0x05 | 0x04 | 0x06 | 0x97  0x98  0x99 | 0x49  0x50  0x51  0x52  0x53  0x54 |
+------+------+------+------+------------------+------------------------------------+
```


This explanation and diagram should help clarify the message format and ensure correct handling of the pairing requests in your UDP relay service.

## Daemon Mode

When run with the `--daemonize` option, the service detaches from the terminal and runs in the background. It will create a PID file in `/tmp` to track the daemon process.

## Troubleshooting

- **Socket Binding Issues:** Ensure no other process is using the configured UDP port.


Happy relaying!
