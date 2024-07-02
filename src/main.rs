use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::process::exit;
use std::str;
use std::sync::{Arc, Mutex};
use std::thread;

use clap::Parser;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// UDP Port for peer A
    port_a: u16,

    /// UDP Port for peer B
    port_b: u16,

    /// The ip to binds
    #[clap(default_value = "0.0.0.0")]
    bind_ip: Ipv4Addr,

    /// Reject unknown sender (only has effect before COUNT)
    #[arg(short, long)]
    no_reject_unknown_sender: bool,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Number of times to allow adjusting peer address after PSK verification
    #[arg(short, long, default_value_t = 1)]
    count: u8,

    /// Pre-shared key
    #[arg(short, long, default_value = "HA12")]
    preshared_key: String,
}

macro_rules! println_if_verbose {
    ($verbose:expr, $($arg:tt)*) => {
        if $verbose {
            println!($($arg)*);
        }
    };
}

fn loop_process_socket<T>(
    socket: &UdpSocket,
    mut on_message_functor: impl FnMut(&[u8], SocketAddr) -> Option<T>,
) -> T {
    // loop untils some value is returned by the functor
    let mut buf = [0u8; 65535];
    loop {
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                if n > 0 {
                    match on_message_functor(&buf[..n], from) {
                        Some(v) => return v,
                        None => {}
                    }
                }
            }
            Err(_) => {}
        }
    }
}

fn wait_for_peers_check_psk(fsrc: &UdpSocket, args: &Args) -> SocketAddr {
    // function that verifies PSK
    let udp = fsrc.local_addr().unwrap().port();
    println_if_verbose!(args.verbose, "> Waiting for PSK @UDP {}", udp);

    let result_addr: SocketAddr = loop_process_socket(&fsrc, |message, from| {
        match str::from_utf8(&message) {
            Ok(v) => {
                if v.trim() == args.preshared_key {
                    println_if_verbose!(args.verbose, "> Got peer info for {}", from);
                    return Some(from);
                }
            }
            Err(_) => {}
        };
        println_if_verbose!(args.verbose, ">> Invalid key @UDP {}, from {}", udp, from);
        None
    });
    result_addr
}

// Worker function to handle UDP packet forwarding
fn worker(
    fsrc: &UdpSocket,
    fdst: &UdpSocket,
    ssrc: Arc<Mutex<SocketAddr>>,
    sdst: Arc<Mutex<SocketAddr>>,
    args: &Args,
) {
    fn _verbose_print_recieved_data(
        message: &[u8],
        from: SocketAddr,
        ssrc: SocketAddr,
        sdst: SocketAddr,
    ) {
        let string = match str::from_utf8(&message) {
            Ok(v) => v,
            Err(_) => &format!("[some bytes of length {}]", message.len()),
        };
        println!("> {} => {}: {} | => {}", from, ssrc, string.trim(), sdst)
    }

    for i in 0..args.count {
        loop_process_socket(&fsrc, |message, from| {
            let mut sender = ssrc.lock().unwrap();
            *sender = from;

            if args.verbose {
                _verbose_print_recieved_data(message, from, *sender, *sdst.lock().unwrap());
                println!(">> Current pre process {}", i)
            }
            drop(sender);
            // let a = sdst.lock().unwrap();

            fdst.send_to(&message, *sdst.lock().unwrap()).unwrap();
            Some(()) // break every time
        })
    }

    // we will now fix it
    let fixed_src_addr: SocketAddr = ssrc.lock().unwrap().clone();
    let fixed_dst_addr: SocketAddr = sdst.lock().unwrap().clone();

    loop_process_socket(&fsrc, |message, from| {
        if !args.no_reject_unknown_sender && fixed_src_addr != from {
            println_if_verbose!(args.verbose, "> Rejected unknown peer {}", from);
            return None;
        }

        if args.verbose {
            _verbose_print_recieved_data(message, from, fixed_src_addr, fixed_dst_addr);
        }
        fdst.send_to(&message, fixed_dst_addr).unwrap();
        None
    })
}

fn bind_or_exit(ip: Ipv4Addr, port: u16) -> UdpSocket {
    match UdpSocket::bind((ip, port)) {
        Ok(socket) => return socket,
        Err(_) => {
            eprintln!("Failed to bind {}:{}", ip, port);
            exit(1);
        }
    }
}

fn main() {
    let args = Args::parse();

    // Create UDP sockets for port A and port B
    let udp_a = bind_or_exit(args.bind_ip, args.port_a);
    let udp_b = bind_or_exit(args.bind_ip, args.port_b);

    // verify pre-shared key
    let peers = thread::scope(|scope| {
        let peer_a = scope.spawn(|| wait_for_peers_check_psk(&udp_a, &args));
        let peer_b = scope.spawn(|| wait_for_peers_check_psk(&udp_b, &args));
        let peer_a = peer_a.join().unwrap();
        let peer_b = peer_b.join().unwrap();
        return (peer_a, peer_b);
    });

    // // Socket addresses for binding
    // let sock_a: SocketAddr = SocketAddr::new(IpAddr::V4(args.bind_ip), args.port_a);
    // let sock_b: SocketAddr = SocketAddr::new(IpAddr::V4(args.bind_ip), args.port_b);

    // //////////////////
    println_if_verbose!(args.verbose, "> Starting bidirectional relay...");

    // Create threads for bidirectional UDP relay
    thread::scope(|scope| {
        let peer_a = Arc::new(Mutex::new(peers.0));
        let peer_b = Arc::new(Mutex::new(peers.1));
        let peer_a_clone: Arc<Mutex<SocketAddr>> = peer_a.clone();
        let peer_b_clone: Arc<Mutex<SocketAddr>> = peer_b.clone();

        let udp_a = &udp_a;
        let udp_b = &udp_b;
        let args = &args;

        scope.spawn(move || worker(&udp_a, &udp_b, peer_a_clone, peer_b_clone, &args));
        scope.spawn(move || worker(&udp_b, &udp_a, peer_b, peer_a, &args));
    });
}
