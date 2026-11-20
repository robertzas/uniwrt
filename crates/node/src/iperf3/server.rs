//! One test on a listener: the handshake, the streams' tasks and the results.

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use ring::rand::{SecureRandom as _, SystemRandom};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    sync::watch,
    task::JoinSet,
    time::{self, Instant},
};

use super::{
    Iperf3Error, Iperf3Summary, ServerConfig, StreamSummary,
    proto::{self, Cookie, MAX_PARAMS, MAX_RESULTS, Params, state},
};

/// How long past its length (and omitted seconds) a test may run before it is ended.
const GRACE: Duration = Duration::from_secs(10);
/// How long the client may take to say it is done once the results are shown.
const DONE_WAIT: Duration = Duration::from_secs(5);
/// The receiving streams' buffer.
const READ_BUFFER: usize = 128 << 10;

/// Takes one test on `listener` from an address `config` allows; answers what it carried.
/// A connection that sends no cookie (or not in time) is closed, and the next one taken.
pub async fn serve(
    listener: TcpListener,
    config: ServerConfig,
) -> Result<Iperf3Summary, Iperf3Error> {
    loop {
        let (mut ctrl, peer) = listener
            .accept()
            .await
            .map_err(Iperf3Error::io("cannot accept a connection"))?;
        if !config.allows(peer.ip()) {
            continue;
        }
        let _ = ctrl.set_nodelay(true);
        let Ok(Ok(cookie)) = time::timeout(config.handshake, proto::read_cookie(&mut ctrl)).await
        else {
            continue;
        };
        return Test {
            listener: &listener,
            config: &config,
            ctrl,
            peer,
            cookie,
        }
        .run()
        .await;
    }
}

struct Test<'a> {
    listener: &'a TcpListener,
    config: &'a ServerConfig,
    ctrl: TcpStream,
    peer: SocketAddr,
    cookie: Cookie,
}

impl Test<'_> {
    async fn run(mut self) -> Result<Iperf3Summary, Iperf3Error> {
        let handshake = self.config.handshake;
        proto::write_state(&mut self.ctrl, state::PARAM_EXCHANGE).await?;
        let value = time::timeout(handshake, proto::read_json(&mut self.ctrl, MAX_PARAMS))
            .await
            .map_err(|_| timeout("send its parameters", handshake))??;
        let params = match Params::parse(&value, self.config.max_seconds, self.config.max_streams) {
            Ok(params) => params,
            Err(why) => {
                let _ = proto::write_state(&mut self.ctrl, state::ACCESS_DENIED).await;
                return Err(Iperf3Error::Protocol(format!("refused: {why}")));
            }
        };
        proto::write_state(&mut self.ctrl, state::CREATE_STREAMS).await?;
        let sockets = self.accept_streams(params.streams()).await?;
        proto::write_state(&mut self.ctrl, state::TEST_START).await?;
        proto::write_state(&mut self.ctrl, state::TEST_RUNNING).await?;
        let started = Instant::now();
        let (stop, stopped) = watch::channel(false);
        let block = random_block(params.block)?;
        let mut tasks = JoinSet::new();
        let mut counters = Vec::new();
        for (index, socket) in (0..).zip(sockets) {
            let counter = Arc::new(AtomicU64::new(0));
            let sends = params.sends(index);
            counters.push((proto::stream_id(index), sends, Arc::clone(&counter)));
            let stopped = stopped.clone();
            if sends {
                tasks.spawn(send(socket, Arc::clone(&block), counter, stopped));
            } else {
                tasks.spawn(receive(socket, counter, stopped));
            }
        }
        let omit_end = started + Duration::from_secs(u64::from(params.omit));
        let deadline = omit_end + Duration::from_secs(u64::from(params.seconds)) + GRACE;
        let ended = self.until_test_end(&counters, omit_end, deadline).await;
        let _ = stop.send(true);
        let stopped_at = Instant::now();
        tasks.shutdown().await;
        let omitted = ended?;
        let seconds = stopped_at.saturating_duration_since(omit_end).as_secs_f64();
        let streams: Vec<StreamSummary> = counters
            .iter()
            .zip(omitted)
            .map(|((id, sent, counter), omitted)| StreamSummary {
                id: *id,
                sent: *sent,
                bytes: counter.load(Ordering::Relaxed).saturating_sub(omitted),
                seconds,
            })
            .collect();
        proto::write_state(&mut self.ctrl, state::EXCHANGE_RESULTS).await?;
        time::timeout(handshake, proto::read_json(&mut self.ctrl, MAX_RESULTS))
            .await
            .map_err(|_| timeout("send its results", handshake))??;
        proto::write_json(&mut self.ctrl, &proto::results(&streams)).await?;
        proto::write_state(&mut self.ctrl, state::DISPLAY_RESULTS).await?;
        // IPERF_DONE, or the connection's end: either way the test is over.
        let mut done = [0; 1];
        let _ = time::timeout(DONE_WAIT, self.ctrl.read(&mut done)).await;
        Ok(Iperf3Summary {
            peer: self.peer,
            streams,
        })
    }

    /// Accepts the test's `count` streams, each opened with the test's cookie; another cookie
    /// is denied, and other addresses closed.
    async fn accept_streams(&mut self, count: u32) -> Result<Vec<TcpStream>, Iperf3Error> {
        let handshake = self.config.handshake;
        let deadline = Instant::now() + handshake;
        let mut sockets = Vec::new();
        let mut byte = [0; 1];
        while sockets.len() < count as usize {
            tokio::select! {
                accepted = self.listener.accept() => {
                    let (mut socket, peer) =
                        accepted.map_err(Iperf3Error::io("cannot accept a stream"))?;
                    if !self.config.allows(peer.ip()) {
                        continue;
                    }
                    match time::timeout_at(deadline, proto::read_cookie(&mut socket)).await {
                        Ok(Ok(cookie)) if cookie == self.cookie => sockets.push(socket),
                        Ok(Ok(_)) => {
                            let _ = socket.write_all(&state::ACCESS_DENIED.to_be_bytes()).await;
                        }
                        Ok(Err(_)) => {}
                        Err(_) => return Err(timeout("open its streams", handshake)),
                    }
                }
                read = self.ctrl.read(&mut byte) => return Err(unexpected(read, byte[0])),
                () = time::sleep_until(deadline) => {
                    return Err(timeout("open its streams", handshake));
                }
            }
        }
        Ok(sockets)
    }

    /// Waits for the client's `TEST_END`; answers each stream's bytes at the end of the omitted
    /// seconds.
    async fn until_test_end(
        &mut self,
        counters: &[(u32, bool, Arc<AtomicU64>)],
        omit_end: Instant,
        deadline: Instant,
    ) -> Result<Vec<u64>, Iperf3Error> {
        let snapshot = || -> Vec<u64> {
            counters
                .iter()
                .map(|(_, _, counter)| counter.load(Ordering::Relaxed))
                .collect()
        };
        // Nothing is omitted without omitted seconds (the streams may have started already).
        let mut omitted: Option<Vec<u64>> =
            (omit_end <= Instant::now()).then(|| vec![0; counters.len()]);
        let mut byte = [0; 1];
        loop {
            tokio::select! {
                read = self.ctrl.read(&mut byte) => {
                    return match read {
                        Ok(1) if i8::from_be_bytes(byte) == state::TEST_END => {
                            // Ended within the omitted seconds: all of it counts.
                            Ok(omitted.unwrap_or_else(|| vec![0; counters.len()]))
                        }
                        other => Err(unexpected(other, byte[0])),
                    };
                }
                () = time::sleep_until(omit_end), if omitted.is_none() => {
                    omitted = Some(snapshot());
                }
                () = time::sleep_until(deadline) => {
                    return Err(Iperf3Error::Protocol(
                        "the test ran past its length: ended".to_owned(),
                    ));
                }
            }
        }
    }
}

/// What a read of the control connection outside its turn means.
fn unexpected(read: std::io::Result<usize>, byte: u8) -> Iperf3Error {
    match read {
        Ok(1) if i8::from_be_bytes([byte]) == state::CLIENT_TERMINATE => Iperf3Error::Terminated,
        Ok(1) => Iperf3Error::Protocol(format!(
            "the client sent state {} out of turn",
            i8::from_be_bytes([byte])
        )),
        Ok(_) => Iperf3Error::Terminated,
        Err(err) => Iperf3Error::io("cannot read from the client")(err),
    }
}

const fn timeout(what: &'static str, after: Duration) -> Iperf3Error {
    Iperf3Error::Timeout { what, after }
}

/// A block of `len` random bytes, sent again and again (as iperf3 does: random data doesn't
/// compress on the way).
fn random_block(len: usize) -> Result<Arc<[u8]>, Iperf3Error> {
    let mut block = vec![0; len];
    SystemRandom::new()
        .fill(&mut block)
        .map_err(|_| Iperf3Error::Protocol("no random data for the stream".to_owned()))?;
    Ok(block.into())
}

async fn receive(
    mut socket: TcpStream,
    counter: Arc<AtomicU64>,
    mut stopped: watch::Receiver<bool>,
) {
    let mut buf = vec![0; READ_BUFFER];
    loop {
        tokio::select! {
            read = socket.read(&mut buf) => match read {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    counter.fetch_add(n as u64, Ordering::Relaxed);
                }
            },
            _ = stopped.wait_for(|stop| *stop) => return,
        }
    }
}

async fn send(
    mut socket: TcpStream,
    block: Arc<[u8]>,
    counter: Arc<AtomicU64>,
    mut stopped: watch::Receiver<bool>,
) {
    let mut at = 0;
    loop {
        tokio::select! {
            written = socket.write(&block[at..]) => match written {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    counter.fetch_add(n as u64, Ordering::Relaxed);
                    at = (at + n) % block.len();
                }
            },
            _ = stopped.wait_for(|stop| *stop) => return,
        }
    }
}
