use std::borrow::BorrowMut;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::process::{exit, ExitCode};
use std::rc::{Rc, Weak};
use std::str;
use std::time::Duration;
use std::time::SystemTime;

use clap::Parser;
use daemonize_me::Daemon;

enum Ops {
    // SYN,
    ACK,
    EstablishConnection,
}

impl Ops {
    fn value(&self) -> [u8; 2] {
        match *self {
            // Ops::SYN => [0xff, 0x02],
            Ops::ACK => [0xff, 0x12],
            Ops::EstablishConnection => [0xff, 0x05],
        }
    }
}

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// UDP Port for peer connection
    udp_port: u16,

    /// The ip to binds
    #[clap(default_value = "0.0.0.0")]
    bind_ip: Ipv4Addr,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Daemonize the process
    #[arg(short, long)]
    daemonize: bool,

    /// Number of seconds before timing out the socket wait. This defines how often would
    /// the relay check for inactivities, and hence, terminates the connection.
    #[arg(short, long, default_value_t = 25)]
    timeout_socket_wait: u64,

    /// Number of seconds before timing out with no connections
    #[arg(long, default_value_t = 300)]
    timeout_no_connections: u64,

    /// Number of seconds before timing out the peer pairing
    #[arg(long, default_value_t = 90)]
    timeout_pairing: u64,

    /// Number of seconds before timing out connection with no activities
    #[arg(long, default_value_t = 180)]
    timeout_connection_inactivities: u64,

    /// Pre-shared key
    #[arg(long, default_value = "uNYDA5QRcvYgp2gfS5v5")]
    preshared_key: String,
}

macro_rules! println_if_verbose {
    ($verbose:expr, $($arg:tt)*) => {
        if $verbose {
            eprintln!($($arg)*);
        }
    };
}

#[derive(Debug)]
struct ExpiringTimer(SystemTime);

impl ExpiringTimer {
    fn access(&mut self) {
        self.0 = SystemTime::now();
    }

    fn is_expired(&self, timeout: u64) -> bool {
        let elapsed = match SystemTime::now().duration_since(self.0) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "Error in getting time elapsed: {}. Defaulting to timeout.",
                    e
                );
                Duration::new(timeout, 0)
            }
        };
        elapsed.as_secs() >= timeout
    }

    fn new() -> ExpiringTimer {
        return ExpiringTimer(SystemTime::now());
    }
}

#[derive(Debug)]
struct Recipient<'a> {
    socket: &'a UdpSocket,
    addr: SocketAddr,
}

impl Recipient<'_> {
    fn send_message(&self, message: &[u8]) {
        self.socket
            .send_to(&message, self.addr)
            .expect("Error in sending message");
    }
}

#[derive(Debug)]
struct RecipientData<'a> {
    recipient: Recipient<'a>,
    last_accessed: ExpiringTimer,
    opponent: Option<Weak<RefCell<RecipientData<'a>>>>,
}

impl<'a> RecipientData<'a> {
    fn get_opponent(&mut self) -> Rc<RefCell<RecipientData<'a>>> {
        self.opponent
            .as_mut()
            .expect("Option is empty. Bugs in setting up opponent?")
            .upgrade()
            .expect("Cannot upgrade to strong reference")
    }
}

fn build_paired_peers<'a>(
    addr_1: &SocketAddr,
    udp_1: &'a UdpSocket,
    addr_2: &SocketAddr,
    udp_2: &'a UdpSocket,
) -> (
    Rc<RefCell<RecipientData<'a>>>,
    Rc<RefCell<RecipientData<'a>>>,
) {
    let peer1 = Rc::new(RefCell::new(RecipientData {
        recipient: Recipient {
            socket: &udp_1,
            addr: addr_1.clone(),
        },
        last_accessed: ExpiringTimer::new(),
        opponent: None,
    }));
    let peer2 = Rc::new(RefCell::new(RecipientData {
        recipient: Recipient {
            socket: &udp_2,
            addr: addr_2.clone(),
        },
        last_accessed: ExpiringTimer::new(),
        opponent: None,
    }));
    // assign the opposing reference as weak pointer

    // peer1.borrow_mut().get_mut().op;

    peer1
        .as_ref()
        .borrow_mut()
        .opponent
        .replace(Rc::downgrade(&peer2));
    peer2
        .as_ref()
        .borrow_mut()
        .opponent
        .replace(Rc::downgrade(&peer1));
    (peer1, peer2)
}

fn bind_socket(ip: Ipv4Addr, port: u16, args: &Args) -> Result<UdpSocket, io::Error> {
    UdpSocket::bind((ip, port)).and_then(|socket| {
        socket
            .set_read_timeout(Some(Duration::new(args.timeout_socket_wait, 0)))
            .ok();
        Ok(socket)
    })
}

fn concat_arrays<T: Copy>(known_array: &[T], borrowed_slice: &[T]) -> Vec<T> {
    let mut combined_array = Vec::with_capacity(known_array.len() + borrowed_slice.len());

    combined_array.extend_from_slice(known_array);
    combined_array.extend_from_slice(borrowed_slice);

    combined_array
}

fn process_relay_service(args: &Args, buffer: &[u8], sender: &Rc<RefCell<RecipientData>>) {
    let mut sender = sender.as_ref().borrow_mut();
    sender.last_accessed.access();
    let receiver = sender.get_opponent();
    let receiver = receiver.as_ref().borrow_mut();
    receiver.recipient.send_message(&buffer);
    println_if_verbose!(
        args.verbose,
        "> Relaying message {} => {} => {}: ",
        sender.recipient.addr,
        str::from_utf8(&buffer).unwrap_or("[some bytes]").trim(),
        receiver.recipient.addr
    );
}

fn process_pairing_request(
    args: &Args,
    registry: &mut RelayService,
    buffer: &[u8],
    from: &SocketAddr,
) {
    let psk_bytes = args.preshared_key.as_bytes();
    // [**xyPPPPP...PPPPPSSSSS....SSSS]
    // *: command
    // x: denote number of bytes (after the first 4 bytes) for PSK
    // y: denote number of bytes (after the first 4 + x bytes) for secret key
    // P: pre-shared key (where len = x)
    // S: Secret key (where len = y)
    if buffer.len() > (2 + args.preshared_key.as_bytes().len()) {
        // check at least it has the minimum number of bytes needed
        if buffer[0..2] == Ops::EstablishConnection.value() {
            println_if_verbose!(args.verbose, "> Got establish connection token from {from}");

            let n_psk: usize = buffer[2].into();
            let psk_end = 4 + n_psk;
            let n_secret: usize = buffer[3].into();
            if buffer.len() < n_psk + n_secret {
                println_if_verbose!(
                    args.verbose,
                    "> Aborting as there aren't enough message length than needed"
                );
                return;
            }

            let peer_secret = &buffer[psk_end..(psk_end + n_secret)];

            if &buffer[4..psk_end] == psk_bytes {
                // send ack
                println_if_verbose!(
                    args.verbose,
                    "> Authenticated. Peer secret: {:?}",
                    str::from_utf8(peer_secret).unwrap_or("[some bytes]")
                );
                match registry.pending_pairing.get_mut(peer_secret) {
                    Some((other_peer, timer)) if other_peer == from => {
                        println_if_verbose!(
                            args.verbose,
                            "> Found existing pairing request from same address/ip/secret. Ignoring..."
                        );
                        timer.access();
                    }
                    Some((_, _)) => {
                        let (other_peer, _) = registry
                            .pending_pairing
                            .remove(peer_secret)
                            .expect("This should exists, as it just were");
                        let (peer1, peer2) = build_paired_peers(
                            &other_peer,
                            &registry.socket,
                            from,
                            &registry.socket,
                        );
                        println_if_verbose!(
                            args.verbose,
                            "> Found other peer with same secret. Connecting {} to {}.",
                            peer1.borrow().recipient.addr,
                            peer2.borrow().recipient.addr,
                        );
                        registry.pairing.insert(other_peer, peer1);
                        registry.pairing.insert(from.clone(), peer2);
                    }
                    None => {
                        let message = concat_arrays(&Ops::ACK.value(), peer_secret);
                        registry
                            .socket
                            .send_to(&message, from)
                            .expect("Error in sending message");

                        registry
                            .pending_pairing
                            .borrow_mut()
                            .insert(peer_secret.to_owned(), (from.clone(), ExpiringTimer::new()));
                    }
                }
            } else {
                println_if_verbose!(args.verbose, "> Aborting as psk does not match");
            }
        }
    }
}

struct RelayService<'a> {
    pairing: HashMap<SocketAddr, Rc<RefCell<RecipientData<'a>>>>,
    pending_pairing: HashMap<Vec<u8>, (SocketAddr, ExpiringTimer)>,
    socket: &'a UdpSocket,
}

impl RelayService<'_> {
    fn is_empty(&self) -> bool {
        self.pairing.len() == 0 && self.pending_pairing.len() == 0
    }

    fn remove_inactive_connections(&mut self, args: &Args) {
        if self.pairing.len() == 0 {
            return;
        }
        // keep track of the pairs of addr to remove.
        let mut to_remove = HashSet::new();
        for (_, peer_a_rc) in &self.pairing {
            let mut peer_a_guard = peer_a_rc.as_ref().borrow_mut();
            let peer_b_rc = peer_a_guard.get_opponent();
            let peer_b_guard = peer_b_rc.as_ref().borrow_mut();

            let last_access_a = &peer_a_guard.last_accessed;
            let last_access_b = &peer_b_guard.last_accessed;

            if last_access_a.is_expired(args.timeout_connection_inactivities)
                && last_access_b.is_expired(args.timeout_connection_inactivities)
            {
                println_if_verbose!(args.verbose, "> Connection between '{addr1}' and '{addr2} has no activities after {timeout} seconds. Removing them...",
                        addr1=peer_a_guard.recipient.addr,
                        addr2=peer_b_guard.recipient.addr,
                        timeout=args.timeout_connection_inactivities
                    );
                to_remove.insert(peer_a_guard.recipient.addr);
                to_remove.insert(peer_b_guard.recipient.addr);
            };
        }

        for k in to_remove {
            self.pairing.remove(&k).expect("unable to remvoe key");
        }
    }

    fn remove_expired_pairing_request(&mut self, args: &Args) {
        self.pending_pairing.retain(|_, (v, pending_timer)| {
            if pending_timer.is_expired(args.timeout_pairing) {
                println_if_verbose!(
                    args.verbose,
                    "> Pending pairing from '{v}' is expired after {} seconds",
                    args.timeout_pairing
                );
                return false;
            }
            true
        });
    }
}

fn start_relay_service(args: &Args, socket: UdpSocket) {
    let mut registry = RelayService {
        pairing: HashMap::new(),
        pending_pairing: HashMap::new(),
        socket: &socket,
    };

    // loop untils some value is returned by the functor
    let mut buf = [0u8; 65535];
    let mut no_connection_since: Option<ExpiringTimer> = None;

    // let psk_bytes = args.preshared_key.as_bytes();
    loop {
        match registry.socket.recv_from(&mut buf) {
            Ok((n, from)) if n > 0 => match registry.pairing.get(&from) {
                Some(sender) => process_relay_service(&args, &buf[..n], &sender),
                None => process_pairing_request(&args, &mut registry, &buf[..n], &from),
            },

            // when this socket timeout, do some processing in the following.
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
            Err(e) => eprintln!("Unexpected error: {e}"),
            _ => (),
        };

        // stop this process when it has no activities after the given time
        match (&no_connection_since, registry.is_empty()) {
            (Some(timer), true) => {
                if timer.is_expired(args.timeout_no_connections) {
                    println_if_verbose!(
                        args.verbose,
                        "> No connections for {} seconds. Quitting...",
                        args.timeout_no_connections
                    );
                    break;
                }
            }
            // remove timer as there's pending connections
            (Some(_), false) => no_connection_since = None,
            // add a pending timer
            (None, true) => no_connection_since = Some(ExpiringTimer::new()),
            (None, false) => (), // all is good
        };

        registry.remove_expired_pairing_request(&args);
        registry.remove_inactive_connections(&args);
    }
}

fn post_fork_parent(_ppid: i32, cpid: i32) -> ! {
    eprintln!("Daeminized process started; pid: {}.", cpid);
    exit(0)
}

fn main() -> ExitCode {
    let args = Args::parse();

    // Create UDP sockets for listening port
    let socket = match bind_socket(args.bind_ip, args.udp_port, &args) {
        Ok(socket) => socket,
        Err(e) => {
            eprintln!("Cannot binds socket: {}", e);
            exit(49)
        }
    };

    if args.daemonize {
        // let stdout = File::create("/tmp/daemon.out").unwrap();
        // let stderr = File::create("/tmp/daemon.err").unwrap();

        let daemon = Daemon::new()
            .pid_file("/tmp/udprelay-rs.pid", Some(false))
            .umask(0o000)
            .work_dir("/tmp")
            // .stdout(stdout)
            // .stderr(stderr)
            // Hooks are optional
            .setup_post_fork_parent_hook(post_fork_parent);

        match daemon.start() {
            Ok(_) => eprintln!("Success, daemonized"),
            Err(e) => {
                eprintln!("Error: {}", e);
                return ExitCode::from(128);
            }
        }
    }
    start_relay_service(&args, socket);

    ExitCode::SUCCESS
}
