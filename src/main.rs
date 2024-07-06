use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::process::exit;
use std::str;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::{error::Error, fmt};

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

    /// Number of seconds before timing out the socket wait
    #[arg(short, long, default_value_t = 5)]
    timeout: u64,

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

#[derive(Debug)]
struct TimeoutError {
    message: String,
    addr: Option<SocketAddr>,
}

impl TimeoutError {
    fn to_message(&self) -> String {
        format!(
            "{}: {}",
            self.message,
            self.addr
                .map_or("[..]".to_owned(), |addr| { addr.to_string() })
        )
    }
}

// impl From<io::Error> for TimeoutError {
//     fn from(error: io::Error) -> Self {
//         TimeoutError {
//             // kind: String::from("parse"),
//             message: error.to_string(),
//         }
//     }
// }

fn loop_process_socket<T>(
    socket: &UdpSocket,
    mut on_message_functor: impl FnMut(&[u8], SocketAddr) -> Option<T>,
) -> Result<T, TimeoutError> {
    // loop untils some value is returned by the functor
    let mut buf = [0u8; 65535];
    loop {
        match socket.recv_from(&mut buf) {
            Ok((n, from)) if n > 0 => match on_message_functor(&buf[..n], from) {
                Some(v) => return Ok(v),
                None => {}
            },
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Err(TimeoutError {
                    message: format!("Timeout reached",),
                    addr: socket.local_addr().ok(),
                });
            }
            _ => {}
        };
    }
}

fn wait_for_peers_check_psk(fsrc: &UdpSocket, args: &Args) -> Result<SocketAddr, TimeoutError> {
    // function that verifies PSK
    let udp = fsrc.local_addr().unwrap().port();
    println_if_verbose!(args.verbose, "> Waiting for PSK @UDP {}", udp);

    loop_process_socket(&fsrc, |message, from| {
        let Ok(v) = str::from_utf8(&message) else {
            return None;
        };
        if v.trim() == args.preshared_key {
            println_if_verbose!(args.verbose, "> Got peer info for {from}");
            return Some(from);
        }
        println_if_verbose!(args.verbose, ">> Invalid key @UDP {udp}, from {from}");
        None
    })
}

struct Peer<'a> {
    socket: &'a UdpSocket,
    addr: Mutex<SocketAddr>,
}
// struct Peer<'a> {
//     socket: &'a UdpSocket,
//     addr: Arc<Mutex<SocketAddr>>,
// }

// Worker function to handle UDP packet forwarding
// fn relay_worker(
//     src: &Peer,
//     dst: &Peer,

//     args: &Args,
// ) -> Result<(), TimeoutError> {
//     let a = relay_worker__(fsrc, fdst, ssrc, sdst, args);
//     println!("exiting... {:?}", fsrc);
//     a
// }
fn relay_worker(src: &Peer, dst: &Peer, args: &Args) -> Result<(), TimeoutError> {
    // helper function to print output
    let _verbose_print_recieved_data = |message: &[u8], from: SocketAddr| {
        if !args.verbose {
            return;
        }
        let message = str::from_utf8(&message)
            .and_then(|m| Ok(m.to_owned()))
            .unwrap_or(format!("[some bytes of length {}]", message.len()));
        println!(
            "> {} => {}: {} | => {}",
            from,
            src.addr.lock().unwrap(),
            message.trim(),
            dst.addr.lock().unwrap()
        );
    };

    for _ in 0..args.count {
        loop_process_socket(&src.socket, |message, from| {
            // update the peer source address as the actual current from's port
            *src.addr.lock().unwrap() = from;

            _verbose_print_recieved_data(message, from);

            dst.socket
                .send_to(&message, *dst.addr.lock().unwrap())
                .unwrap();
            Some(()) // break every time
        })?
    }
    println_if_verbose!(args.verbose, ">> Locking adress");

    // we will now fix the address
    let fixed_src_addr: SocketAddr = src.addr.lock().unwrap().clone();
    let fixed_dst_addr: SocketAddr = dst.addr.lock().unwrap().clone();

    loop_process_socket(&src.socket, |message, from| {
        if !args.no_reject_unknown_sender && fixed_src_addr != from {
            println_if_verbose!(args.verbose, "> Rejected unknown peer {}", from);
            return None;
        }

        _verbose_print_recieved_data(message, from);
        dst.socket.send_to(&message, fixed_dst_addr).unwrap();
        None
    })
}

fn bind_socket(ip: Ipv4Addr, port: u16, args: &Args) -> Result<UdpSocket, io::Error> {
    UdpSocket::bind((ip, port)).and_then(|socket| {
        socket
            .set_read_timeout(Some(Duration::new(args.timeout, 0)))
            .ok();
        Ok(socket)
    })
}

fn main() {
    let args = Args::parse();

    // Create UDP sockets for port A and port B
    let udp_a = bind_socket(args.bind_ip, args.port_a, &args).expect("cannot binds socket");
    let udp_b = bind_socket(args.bind_ip, args.port_b, &args).expect("cannot binds socket");

    // // verify pre-shared key
    // let peers = thread::scope(|scope| {
    //     let peer_a = scope.spawn(|| wait_for_peers_check_psk(&udp_a, &args));
    //     let peer_b = scope.spawn(|| wait_for_peers_check_psk(&udp_b, &args));

    //     // let peer_a = peer_a.join().unwrap().ok().unwrap();
    //     let peer_a = match peer_a.join().unwrap() {
    //         Ok(v) => v,
    //         Err(e) => {
    //             print!("{:?}", e);
    //             panic!("i------------");
    //         }
    //     };

    //     let peer_b = peer_b.join().unwrap().ok().unwrap();
    //     return (peer_a, peer_b);
    // });

    let peers = (
        SocketAddr::new(IpAddr::V4(args.bind_ip), args.port_a),
        SocketAddr::new(IpAddr::V4(args.bind_ip), args.port_b),
    );

    // // Socket addresses for binding
    // let sock_a: SocketAddr = SocketAddr::new(IpAddr::V4(args.bind_ip), args.port_a);
    // let sock_b: SocketAddr = SocketAddr::new(IpAddr::V4(args.bind_ip), args.port_b);

    // //////////////////
    println_if_verbose!(args.verbose, "> Starting bidirectional relay...");

    // Create threads for bidirectional UDP relay
    thread::scope(|scope| {
        let join_thread_handle = |handle: thread::ScopedJoinHandle<Result<(), TimeoutError>>| {
            if let Err(e) = handle.join().unwrap() {
                println_if_verbose!(args.verbose, "{}: ", e.to_message());
            }
        };

        let mut handles = Vec::new();

        let args = &args;
        for _ in 1..5 {
            let peer_a = Arc::new(Peer {
                socket: &udp_a,
                addr: Mutex::new(peers.0),
            });
            let peer_b = Arc::new(Peer {
                socket: &udp_b,
                addr: Mutex::new(peers.1),
            });
            let peer_a_clone = peer_a.clone();
            let peer_b_clone = peer_b.clone();
            handles.push(scope.spawn(move || relay_worker(&peer_a_clone, &peer_b_clone, args)));
            handles.push(scope.spawn(move || relay_worker(&peer_b, &peer_a, args)));
        }

        while handles.len() > 0 {
            // join_thread_handle(handles.remove(0));

            if let Err(e) = handles.remove(0).join().unwrap() {
                println_if_verbose!(args.verbose, "{}: ", e.to_message());
            }
        }
        // {
        //     let peer_a = Arc::new(Peer {
        //         socket: &udp_a,
        //         addr: Mutex::new(peers.0),
        //     });
        //     let peer_b = Arc::new(Peer {
        //         socket: &udp_b,
        //         addr: Mutex::new(peers.1),
        //     });
        //     let peer_a_clone = peer_a.clone();
        //     let peer_b_clone = peer_b.clone();
        //     scope.spawn(move || relay_worker(&peer_a_clone, &peer_b_clone, args));
        //     scope.spawn(move || relay_worker(&peer_b, &peer_a, args));
        // }
        // {
        //     let peer_a = Arc::new(Peer {
        //         socket: &udp_a,
        //         addr: Mutex::new(peers.0),
        //     });
        //     let peer_b = Arc::new(Peer {
        //         socket: &udp_b,
        //         addr: Mutex::new(peers.1),
        //     });
        //     let peer_a_clone = peer_a.clone();
        //     let peer_b_clone = peer_b.clone();
        //     scope.spawn(move || relay_worker(&peer_a_clone, &peer_b_clone, args));
        //     scope.spawn(move || relay_worker(&peer_b, &peer_a, args));
        // }

        // let peer_a_clone = peer_a.clone();
        // let peer_b_clone = peer_b.clone();
        // scope.spawn(move || relay_worker(&udp_a, &udp_b, peer_a_clone, peer_b_clone, &args));
        // let peer_a_clone = peer_a.clone();
        // let peer_b_clone = peer_b.clone();
        // scope.spawn(move || relay_worker(&udp_a, &udp_b, peer_a_clone, peer_b_clone, &args));
    });
    println!("end");
}
