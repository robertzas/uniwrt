//! The receiver: syslog over UDP and TCP, each message parsed and handed on with its sender's
//! address. It never makes a sender wait: a sender over its rate, or a full queue, loses the
//! message, and the losses are counted.

use std::{
    collections::HashMap,
    io,
    net::{IpAddr, SocketAddr},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{Semaphore, mpsc, watch},
    task::JoinHandle,
    time::Instant,
};
use tracing::{debug, warn};
use uniwrt_core::{Clock, Timestamp};

use crate::{Framer, Message, parse};

#[derive(Debug, Clone)]
pub struct ReceiverConfig {
    /// Where to listen for UDP; `None` for not at all.
    pub udp: Option<SocketAddr>,
    /// Where to listen for TCP; `None` for not at all.
    pub tcp: Option<SocketAddr>,
    /// TCP connections at once; more are closed as they come.
    pub max_connections: usize,
    /// Messages per second a sender may send on average, and in a burst.
    pub rate: u32,
    pub burst: u32,
    /// Messages waiting for the consumer; more are dropped.
    pub queue: usize,
}

impl Default for ReceiverConfig {
    fn default() -> Self {
        let any = SocketAddr::from(([0, 0, 0, 0], 5514));
        Self {
            udp: Some(any),
            tcp: Some(any),
            max_connections: 64,
            rate: 200,
            burst: 2_000,
            queue: 4_096,
        }
    }
}

/// What the receiver hands on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Received {
    Message {
        peer: IpAddr,
        /// When it arrived, on the controller's clock.
        at: Timestamp,
        /// Whether it came over TCP (the sender says when it stops).
        tcp: bool,
        message: Message,
    },
    /// A TCP sender disconnected.
    Closed { peer: IpAddr },
}

/// Listens until stopped.
#[derive(Debug)]
pub struct Receiver {
    udp: Option<SocketAddr>,
    tcp: Option<SocketAddr>,
    stop: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
    dropped: Arc<AtomicU64>,
}

/// Shared by the listeners.
struct Shared {
    tx: mpsc::Sender<Received>,
    clock: Arc<dyn Clock>,
    limiter: Mutex<Limiter>,
    dropped: Arc<AtomicU64>,
    /// When the last warning about drops went out.
    warned: Mutex<Option<Instant>>,
}

/// How often drops are reported in the log at most.
const WARN_EVERY: Duration = Duration::from_mins(1);

impl Receiver {
    /// Binds the listeners `config` names, and returns the messages as they arrive.
    pub async fn bind(
        config: &ReceiverConfig,
        clock: Arc<dyn Clock>,
    ) -> io::Result<(Self, mpsc::Receiver<Received>)> {
        let (tx, rx) = mpsc::channel(config.queue.max(1));
        let dropped = Arc::new(AtomicU64::new(0));
        let shared = Arc::new(Shared {
            tx,
            clock,
            limiter: Mutex::new(Limiter::new(config.rate, config.burst)),
            dropped: Arc::clone(&dropped),
            warned: Mutex::new(None),
        });
        let (stop, stopped) = watch::channel(false);
        let mut tasks = Vec::new();
        let mut udp_addr = None;
        if let Some(addr) = config.udp {
            let socket = UdpSocket::bind(addr).await?;
            udp_addr = Some(socket.local_addr()?);
            tasks.push(tokio::spawn(udp(
                socket,
                Arc::clone(&shared),
                stopped.clone(),
            )));
        }
        let mut tcp_addr = None;
        if let Some(addr) = config.tcp {
            let listener = TcpListener::bind(addr).await?;
            tcp_addr = Some(listener.local_addr()?);
            let slots = Arc::new(Semaphore::new(config.max_connections));
            tasks.push(tokio::spawn(tcp(listener, slots, shared, stopped)));
        }
        Ok((
            Self {
                udp: udp_addr,
                tcp: tcp_addr,
                stop,
                tasks,
                dropped,
            },
            rx,
        ))
    }

    pub fn udp_addr(&self) -> Option<SocketAddr> {
        self.udp
    }

    pub fn tcp_addr(&self) -> Option<SocketAddr> {
        self.tcp
    }

    /// Messages lost so far: over a sender's rate, or to a full queue.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Stops listening and closes the connections.
    pub async fn stop(self) {
        let _ = self.stop.send(true);
        for task in self.tasks {
            let _ = task.await;
        }
    }
}

impl Shared {
    fn deliver(&self, peer: IpAddr, tcp: bool, bytes: &[u8]) {
        let at = self.clock.now();
        let Some(message) = parse(bytes, at) else {
            return;
        };
        let allowed = self
            .limiter
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .allow(peer, Instant::now());
        let sent = allowed
            && self
                .tx
                .try_send(Received::Message {
                    peer,
                    at,
                    tcp,
                    message,
                })
                .is_ok();
        if !sent {
            let total = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
            let mut warned = self.warned.lock().unwrap_or_else(PoisonError::into_inner);
            let now = Instant::now();
            if warned.is_none_or(|at| now.duration_since(at) >= WARN_EVERY) {
                *warned = Some(now);
                warn!(%peer, total, "syslog messages dropped: a sender over its rate, or too many to store");
            }
        }
    }
}

async fn udp(socket: UdpSocket, shared: Arc<Shared>, mut stopped: watch::Receiver<bool>) {
    let mut buf = vec![0; 65_536];
    loop {
        tokio::select! {
            got = socket.recv_from(&mut buf) => match got {
                Ok((len, from)) => shared.deliver(from.ip().to_canonical(), false, &buf[..len]),
                Err(err) => debug!(%err, "syslog UDP receive failed"),
            },
            _ = stopped.changed() => return,
        }
    }
}

async fn tcp(
    listener: TcpListener,
    slots: Arc<Semaphore>,
    shared: Arc<Shared>,
    mut stopped: watch::Receiver<bool>,
) {
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, from)) => {
                    let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
                        debug!(%from, "syslog TCP connection refused: too many");
                        continue;
                    };
                    let shared = Arc::clone(&shared);
                    let stopped = stopped.clone();
                    connections.spawn(async move {
                        connection(stream, from.ip().to_canonical(), &shared, stopped).await;
                        drop(slot);
                    });
                }
                Err(err) => {
                    debug!(%err, "syslog TCP accept failed");
                    // Out of descriptors, say: let some close.
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            Some(_) = connections.join_next() => {}
            _ = stopped.changed() => break,
        }
    }
    connections.shutdown().await;
}

async fn connection(
    mut stream: TcpStream,
    peer: IpAddr,
    shared: &Shared,
    mut stopped: watch::Receiver<bool>,
) {
    let mut framer = Framer::new();
    let mut buf = vec![0; 16 * 1024];
    loop {
        tokio::select! {
            read = stream.read(&mut buf) => match read {
                Ok(0) | Err(_) => break,
                Ok(len) => {
                    for frame in framer.push(&buf[..len]) {
                        shared.deliver(peer, true, &frame);
                    }
                }
            },
            _ = stopped.changed() => return,
        }
    }
    let _ = shared.tx.try_send(Received::Closed { peer });
}

/// A token bucket per sender.
#[derive(Debug)]
struct Limiter {
    rate: f64,
    burst: f64,
    buckets: HashMap<IpAddr, (f64, Instant)>,
}

/// Senders tracked before the idle ones are forgotten.
const MAX_SENDERS: usize = 1_024;

impl Limiter {
    fn new(rate: u32, burst: u32) -> Self {
        Self {
            rate: f64::from(rate),
            burst: f64::from(burst.max(1)),
            buckets: HashMap::new(),
        }
    }

    fn allow(&mut self, peer: IpAddr, now: Instant) -> bool {
        if self.buckets.len() >= MAX_SENDERS && !self.buckets.contains_key(&peer) {
            // A full bucket is as good as none.
            let (rate, burst) = (self.rate, self.burst);
            self.buckets.retain(|_, (tokens, at)| {
                *tokens + now.duration_since(*at).as_secs_f64() * rate < burst
            });
        }
        let (tokens, at) = self.buckets.entry(peer).or_insert((self.burst, now));
        *tokens = (*tokens + now.duration_since(*at).as_secs_f64() * self.rate).min(self.burst);
        *at = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{net::IpAddr, sync::Arc, time::Duration};

    use tokio::{
        io::AsyncWriteExt,
        net::{TcpStream, UdpSocket},
        time::Instant,
    };
    use uniwrt_core::{ManualClock, Timestamp};

    use super::{Limiter, Received, Receiver, ReceiverConfig};

    fn config() -> ReceiverConfig {
        let local = "127.0.0.1:0".parse().ok();
        ReceiverConfig {
            udp: local,
            tcp: local,
            ..ReceiverConfig::default()
        }
    }

    async fn next(rx: &mut tokio::sync::mpsc::Receiver<Received>) -> Received {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn udp_and_tcp_messages_arrive_with_their_sender() {
        let at = Timestamp::from_unix_ms(1_800_000_000_000);
        let (receiver, mut rx) = Receiver::bind(&config(), Arc::new(ManualClock::new(at)))
            .await
            .unwrap();
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        udp.send_to(
            b"<30>Oct 29 03:40:30 hostapd: hello",
            receiver.udp_addr().unwrap(),
        )
        .await
        .unwrap();
        let Received::Message {
            peer,
            at: got,
            tcp,
            message,
        } = next(&mut rx).await
        else {
            panic!("a message first");
        };
        assert_eq!(peer, IpAddr::from([127, 0, 0, 1]));
        assert_eq!((got, tcp), (at, false));
        assert_eq!(message.entry.message, "hello");

        let mut stream = TcpStream::connect(receiver.tcp_addr().unwrap())
            .await
            .unwrap();
        stream
            .write_all(b"<13>a: one\n10 <13>b: two")
            .await
            .unwrap();
        for text in ["one", "two"] {
            let Received::Message { tcp, message, .. } = next(&mut rx).await else {
                panic!("a message");
            };
            assert!(tcp);
            assert_eq!(message.entry.message, text);
        }
        drop(stream);
        assert_eq!(
            next(&mut rx).await,
            Received::Closed {
                peer: IpAddr::from([127, 0, 0, 1])
            }
        );
        assert_eq!(receiver.dropped(), 0);
        receiver.stop().await;
    }

    #[tokio::test]
    async fn a_full_queue_drops_and_counts() {
        let config = ReceiverConfig {
            queue: 2,
            ..config()
        };
        let (receiver, mut rx) = Receiver::bind(
            &config,
            Arc::new(ManualClock::new(Timestamp::from_unix_ms(0))),
        )
        .await
        .unwrap();
        let mut stream = TcpStream::connect(receiver.tcp_addr().unwrap())
            .await
            .unwrap();
        stream
            .write_all(b"<13>1\n<13>2\n<13>3\n<13>4\n")
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while receiver.dropped() < 2 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert!(matches!(next(&mut rx).await, Received::Message { .. }));
        receiver.stop().await;
    }

    #[test]
    fn senders_get_their_rate_and_burst() {
        let mut limiter = Limiter::new(10, 3);
        let a = IpAddr::from([10, 0, 0, 1]);
        let b = IpAddr::from([10, 0, 0, 2]);
        let start = Instant::now();
        let allowed = (0..5).filter(|_| limiter.allow(a, start)).count();
        assert_eq!(allowed, 3);
        // Another sender has its own bucket.
        assert!(limiter.allow(b, start));
        // 10 per second: one more after 100 ms.
        let later = start + Duration::from_millis(100);
        assert!(limiter.allow(a, later));
        assert!(!limiter.allow(a, later));
    }
}
