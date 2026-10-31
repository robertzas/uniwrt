//! An in-process SSH server for the transport tests: russh on loopback that runs commands with
//! the local `sh` as dropbear does on a node, plus a built-in fake agent (`fake-agent`).

#![allow(dead_code, unreachable_pub)] // each test binary uses part of it

use std::{
    future::ready,
    net::SocketAddr,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use russh::{
    Channel, ChannelId, ChannelMsg, ChannelOpenFailure, ChannelWriteHalf, Disconnect,
    keys::{PrivateKey, PublicKey, ssh_key::private::Ed25519Keypair},
    server::{self, Auth, ChannelOpenHandle, Msg, Session},
};
use serde_json::{Value, json};
use tokio::{
    io::AsyncWriteExt as _,
    net::{TcpListener, TcpStream},
    process::Command,
    task::JoinHandle,
};
use uniwrt_node::{ControllerKey, HostKey, Target};

pub const LONG: Duration = Duration::from_secs(10);

pub struct TestServer {
    addr: SocketAddr,
    host_key: HostKey,
    shared: Arc<Shared>,
    accept: JoinHandle<()>,
}

#[derive(Default)]
struct Shared {
    authorized: Mutex<Vec<PublicKey>>,
    password: Mutex<Option<String>>,
    sessions: Mutex<Vec<server::Handle>>,
    logins: AtomicUsize,
    refuse: AtomicBool,
}

impl TestServer {
    /// A server whose ed25519 host key derives from `seed`.
    pub async fn start(seed: u8) -> Self {
        let key = PrivateKey::from(Ed25519Keypair::from_seed(&[seed; 32]));
        let host_key = HostKey::parse(&key.public_key().to_openssh().unwrap()).unwrap();
        let config = Arc::new(server::Config {
            keys: vec![key],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..server::Config::default()
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let shared = Arc::new(Shared::default());
        let accept = tokio::spawn(accept(listener, config, Arc::clone(&shared)));
        Self {
            addr,
            host_key,
            shared,
            accept,
        }
    }

    pub fn target(&self) -> Target {
        Target::new("127.0.0.1", self.addr.port())
    }

    pub fn host_key(&self) -> HostKey {
        self.host_key.clone()
    }

    pub fn authorize(&self, key: &ControllerKey) {
        let key = PublicKey::from_openssh(&key.authorized_key().unwrap()).unwrap();
        self.shared.authorized.lock().unwrap().push(key);
    }

    pub fn set_password(&self, password: &str) {
        *self.shared.password.lock().unwrap() = Some(password.to_owned());
    }

    /// Successful logins so far.
    pub fn logins(&self) -> usize {
        self.shared.logins.load(Ordering::SeqCst)
    }

    /// Cuts every connection, as a node reboot would.
    pub async fn drop_connections(&self) {
        let sessions = std::mem::take(&mut *self.shared.sessions.lock().unwrap());
        for session in sessions {
            let _ = session
                .disconnect(Disconnect::ByApplication, "bye".to_owned(), "en".to_owned())
                .await;
        }
    }

    /// While set, connections are closed before the handshake.
    pub fn refuse(&self, refuse: bool) {
        self.shared.refuse.store(refuse, Ordering::SeqCst);
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

/// A server with a fresh controller key authorized, and that key.
pub async fn server_with_key(seed: u8) -> (TestServer, ControllerKey) {
    let server = TestServer::start(seed).await;
    let key = ControllerKey::generate().unwrap();
    server.authorize(&key);
    (server, key)
}

async fn accept(listener: TcpListener, config: Arc<server::Config>, shared: Arc<Shared>) {
    while let Ok((socket, _)) = listener.accept().await {
        if shared.refuse.load(Ordering::SeqCst) {
            continue;
        }
        let config = Arc::clone(&config);
        let shared = Arc::clone(&shared);
        tokio::spawn(async move {
            let handler = Connection {
                shared: Arc::clone(&shared),
            };
            let Ok(running) = server::run_stream(config, socket, handler).await else {
                return;
            };
            shared.sessions.lock().unwrap().push(running.handle());
            let _ = running.await;
        });
    }
}

struct Connection {
    shared: Arc<Shared>,
}

impl Connection {
    fn login(&self, ok: bool) -> Auth {
        if ok {
            self.shared.logins.fetch_add(1, Ordering::SeqCst);
            Auth::Accept
        } else {
            Auth::reject()
        }
    }
}

impl server::Handler for Connection {
    type Error = russh::Error;

    fn auth_publickey(
        &mut self,
        user: &str,
        key: server::VerifiedPublicKey<'_>,
    ) -> impl Future<Output = Result<Auth, Self::Error>> + Send {
        let known = self
            .shared
            .authorized
            .lock()
            .unwrap()
            .iter()
            .any(|authorized| authorized.key_data() == key.public_key.key_data());
        ready(Ok(self.login(user == "root" && known)))
    }

    fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> impl Future<Output = Result<Auth, Self::Error>> + Send {
        let right = self.shared.password.lock().unwrap().as_deref() == Some(password);
        ready(Ok(self.login(user == "root" && right)))
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        tokio::spawn(serve(channel));
        Ok(())
    }

    fn exec_request(
        &mut self,
        channel: ChannelId,
        _: &[u8],
        session: &mut Session,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        ready(session.channel_success(channel))
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        let port = u16::try_from(port).unwrap_or(0);
        match TcpStream::connect((host, port)).await {
            Ok(mut socket) => {
                reply.accept().await;
                tokio::spawn(async move {
                    let mut stream = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut stream, &mut socket).await;
                });
            }
            Err(_) => reply.reject(ChannelOpenFailure::ConnectFailed).await,
        }
        Ok(())
    }
}

async fn serve(mut channel: Channel<Msg>) {
    let command = loop {
        match channel.wait().await {
            Some(ChannelMsg::Exec { command, .. }) => {
                break String::from_utf8_lossy(&command).into_owned();
            }
            Some(_) => {}
            None => return,
        }
    };
    let mut words = command.split_whitespace();
    if words.next() == Some("fake-agent") {
        fake_agent(channel, words.next()).await;
    } else {
        sh(channel, &command).await;
    }
}

/// Runs `command` with `sh -c`, like dropbear with an ash login shell.
async fn sh(channel: Channel<Msg>, command: &str) {
    let (mut read, write) = channel.split();
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let feed = async {
        while let Some(message) = read.wait().await {
            match message {
                ChannelMsg::Data { data } => {
                    if let Some(pipe) = stdin.as_mut()
                        && pipe.write_all(&data).await.is_err()
                    {
                        stdin = None;
                    }
                }
                ChannelMsg::Eof => stdin = None,
                ChannelMsg::Close => break,
                _ => {}
            }
        }
    };
    let run = async {
        let _ = tokio::join!(write.data(stdout), write.extended_data(1, stderr));
        child.wait().await
    };
    tokio::select! {
        status = run => {
            let code = status.ok().and_then(|status| status.code()).unwrap_or(255);
            finish(&write, u32::try_from(code).unwrap_or(255)).await;
        }
        // The client closed the channel: the child is killed as it drops.
        () = feed => {}
    }
}

async fn finish(write: &ChannelWriteHalf<Msg>, code: u32) {
    let _ = write.exit_status(code).await;
    let _ = write.eof().await;
    let _ = write.close().await;
}

/// Says hello (protocol 99 with `--proto-99`), then answers: `echo` returns its args, `emit`
/// pushes an event first, `exit` prints `bye` to stderr and exits with 3.
async fn fake_agent(channel: Channel<Msg>, flag: Option<&str>) {
    let (mut read, write) = channel.split();
    let send = |value: Value| write.data_bytes(format!("{value}\n"));
    let proto = if flag == Some("--proto-99") { 99 } else { 1 };
    let _ = send(json!({"hello": {"proto": proto, "agent": "fake", "release": "25.12.6"}})).await;
    let mut buf = Vec::new();
    while let Some(message) = read.wait().await {
        match message {
            ChannelMsg::Data { data } => buf.extend_from_slice(&data),
            ChannelMsg::Eof | ChannelMsg::Close => break,
            _ => continue,
        }
        while let Some(end) = buf.iter().position(|&byte| byte == b'\n') {
            let line: Vec<u8> = buf.drain(..=end).collect();
            let request: Value = serde_json::from_slice(&line).unwrap();
            let id = &request["id"];
            let answer = match request["op"].as_str().unwrap_or_default() {
                "echo" => json!({"id": id, "ok": request["args"]}),
                "emit" => {
                    let event = json!({"ev": "test.event", "ts": 1, "data": request["args"]});
                    let _ = send(event).await;
                    json!({"id": id, "ok": {}})
                }
                "exit" => {
                    let _ = write.extended_data_bytes(1, "bye\n").await;
                    finish(&write, 3).await;
                    return;
                }
                op => json!({"id": id, "err": {"code": "unknown_op", "msg": op}}),
            };
            let _ = send(answer).await;
        }
    }
    finish(&write, 0).await;
}
