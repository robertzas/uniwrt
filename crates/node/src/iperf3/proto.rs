//! iperf3's wire format: the state bytes, the cookie and JSON messages behind a 4-byte
//! big-endian length (`JSON_write`), the client's parameters and the server's results.

use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

use super::Iperf3Error;

/// The cookie's length: 36 characters and a NUL.
pub(super) const COOKIE_SIZE: usize = 37;
/// The longest parameters taken (iperf3's own bound).
pub(super) const MAX_PARAMS: u32 = 8 << 10;
/// The longest results taken.
pub(super) const MAX_RESULTS: u32 = 64 << 10;
/// The block size when the client doesn't name one (iperf3's TCP default).
pub(super) const DEFAULT_BLOCK: usize = 128 << 10;
/// The largest block sent.
pub(super) const MAX_BLOCK: usize = 1 << 20;

/// The control states (`iperf_api.h`), sent as one signed byte.
pub(super) mod state {
    pub(in crate::iperf3) const TEST_START: i8 = 1;
    pub(in crate::iperf3) const TEST_RUNNING: i8 = 2;
    pub(in crate::iperf3) const TEST_END: i8 = 4;
    pub(in crate::iperf3) const PARAM_EXCHANGE: i8 = 9;
    pub(in crate::iperf3) const CREATE_STREAMS: i8 = 10;
    pub(in crate::iperf3) const CLIENT_TERMINATE: i8 = 12;
    pub(in crate::iperf3) const EXCHANGE_RESULTS: i8 = 13;
    pub(in crate::iperf3) const DISPLAY_RESULTS: i8 = 14;
    #[cfg(test)]
    pub(in crate::iperf3) const IPERF_DONE: i8 = 16;
    pub(in crate::iperf3) const ACCESS_DENIED: i8 = -1;
}

pub(super) type Cookie = [u8; COOKIE_SIZE];

pub(super) async fn read_cookie<R: AsyncRead + Unpin>(from: &mut R) -> std::io::Result<Cookie> {
    let mut cookie = [0; COOKIE_SIZE];
    from.read_exact(&mut cookie).await?;
    Ok(cookie)
}

pub(super) async fn write_state<W: AsyncWrite + Unpin>(
    to: &mut W,
    state: i8,
) -> Result<(), Iperf3Error> {
    to.write_all(&state.to_be_bytes())
        .await
        .map_err(Iperf3Error::io("cannot write to the client"))
}

/// A JSON message of at most `max` bytes.
pub(super) async fn read_json<R: AsyncRead + Unpin>(
    from: &mut R,
    max: u32,
) -> Result<Value, Iperf3Error> {
    let len = from
        .read_u32()
        .await
        .map_err(Iperf3Error::io("cannot read from the client"))?;
    if len > max {
        return Err(Iperf3Error::Protocol(format!(
            "the client sent {len} bytes of JSON, more than the {max} taken"
        )));
    }
    let mut data = vec![0; len as usize];
    from.read_exact(&mut data)
        .await
        .map_err(Iperf3Error::io("cannot read from the client"))?;
    serde_json::from_slice(&data).map_err(|err| {
        Iperf3Error::Protocol(format!("the client sent JSON that doesn't parse: {err}"))
    })
}

pub(super) async fn write_json<W: AsyncWrite + Unpin>(
    to: &mut W,
    value: &Value,
) -> Result<(), Iperf3Error> {
    let data = value.to_string();
    let len = u32::try_from(data.len())
        .map_err(|_| Iperf3Error::Protocol("the results are too long".to_owned()))?;
    let mut message = Vec::with_capacity(4 + data.len());
    message.extend_from_slice(&len.to_be_bytes());
    message.extend_from_slice(data.as_bytes());
    to.write_all(&message)
        .await
        .map_err(Iperf3Error::io("cannot write to the client"))
}

/// Which way a test's streams carry data, seen from the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    /// The client sends.
    Receive,
    /// The client receives (`-R`).
    Send,
    /// The first half of the streams send to the server, the second half receive (`--bidir`).
    Both,
}

/// What the client asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Params {
    pub(super) mode: Mode,
    /// Streams per direction.
    pub(super) parallel: u32,
    pub(super) seconds: u32,
    pub(super) omit: u32,
    pub(super) block: usize,
}

impl Params {
    /// The streams the client opens.
    pub(super) const fn streams(&self) -> u32 {
        match self.mode {
            Mode::Both => self.parallel * 2,
            Mode::Receive | Mode::Send => self.parallel,
        }
    }

    /// Whether the server sends on the `index`th stream opened.
    pub(super) const fn sends(&self, index: u32) -> bool {
        match self.mode {
            Mode::Receive => false,
            Mode::Send => true,
            Mode::Both => index >= self.parallel,
        }
    }

    /// The parameters in `value`, if the server runs them: TCP, timed, within the bounds.
    pub(super) fn parse(value: &Value, max_seconds: u32, max_streams: u32) -> Result<Self, String> {
        let flag = |name: &str| match &value[name] {
            Value::Bool(set) => *set,
            Value::Number(n) => n.as_i64().is_some_and(|n| n != 0),
            _ => false,
        };
        let number = |name: &str| value[name].as_u64().unwrap_or(0);
        if flag("udp") || flag("sctp") {
            return Err("only TCP tests are taken".to_owned());
        }
        if number("num") != 0 || number("blockcount") != 0 {
            return Err("only timed tests are taken (-t, not -n or -k)".to_owned());
        }
        let mode = match (flag("reverse"), flag("bidirectional")) {
            (_, true) => Mode::Both,
            (true, false) => Mode::Send,
            (false, false) => Mode::Receive,
        };
        let parallel = u32::try_from(number("parallel")).unwrap_or(u32::MAX).max(1);
        let seconds = u32::try_from(number("time")).unwrap_or(u32::MAX);
        if seconds == 0 || seconds > max_seconds {
            return Err(format!("a test lasts 1 to {max_seconds} seconds"));
        }
        let params = Self {
            mode,
            parallel,
            seconds,
            omit: u32::try_from(number("omit")).unwrap_or(u32::MAX),
            block: match usize::try_from(number("len")).unwrap_or(usize::MAX) {
                0 => DEFAULT_BLOCK,
                len => len.min(MAX_BLOCK),
            },
        };
        if params.streams() > max_streams {
            return Err(format!("a test runs at most {max_streams} streams"));
        }
        if params.omit > 10 {
            return Err("a test omits at most 10 seconds".to_owned());
        }
        Ok(params)
    }
}

/// iperf3's id of the `index`th stream opened: 1, 3, 4, … (`iperf_add_stream` skips 2).
pub(super) const fn stream_id(index: u32) -> u32 {
    if index == 0 { 1 } else { index + 2 }
}

/// The server's results: each stream's bytes and length. iperf3 needs every field `get_results`
/// reads; the CPU's use and retransmits aren't measured (`-1`: none known).
pub(super) fn results(streams: &[super::StreamSummary]) -> Value {
    let sends = streams.iter().any(|stream| stream.sent);
    json!({
        "cpu_util_total": 0,
        "cpu_util_user": 0,
        "cpu_util_system": 0,
        "sender_has_retransmits": if sends { 0 } else { -1 },
        "streams": streams.iter().map(|stream| json!({
            "id": stream.id,
            "bytes": stream.bytes,
            "retransmits": -1,
            "jitter": 0,
            "errors": 0,
            "omitted_errors": 0,
            "packets": 0,
            "omitted_packets": 0,
            "start_time": 0,
            "end_time": stream.seconds,
        })).collect::<Vec<_>>(),
    })
}
