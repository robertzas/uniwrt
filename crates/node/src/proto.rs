//! The agent protocol's envelope (ARCHITECTURE.md, Node protocol): one JSON object per line in
//! each direction. The controller sends requests; the agent answers each by its id and pushes
//! events and ticks unasked. Its first line is a [`Hello`]. The ops, their arguments and results
//! are in [`ops`]; `node-agent/proto.uc` and `node-agent/ops.uc` mirror both files.
//!
//! ```jsonc
//! {"id":7,"op":"collect","args":{"sets":["system"]}}           // controller → agent
//! {"id":7,"ok":{…}}  {"id":9,"err":{"code":"timeout","msg":"…"}} // answers
//! {"ev":"hostapd.assoc","ts":1761091200123,"data":{…}}           // an event
//! {"tick":{"system":{…}}}                                        // periodic collection
//! ```

pub mod ops;

use std::{ops::RangeInclusive, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use uniwrt_core::Timestamp;

/// The protocol version this controller speaks. It bumps on breaking changes.
pub const PROTO: u32 = 1;
/// The versions accepted from an agent: the current one and the previous one, so the start
/// moves to `PROTO - 1` with the first bump (v1 is the first).
pub const SUPPORTED: RangeInclusive<u32> = 1..=PROTO;
/// The longest line accepted from an agent.
pub const MAX_LINE: usize = 16 << 20;

/// A request, as the controller writes it.
#[derive(Debug, Serialize)]
pub(crate) struct Request<'a> {
    pub(crate) id: u64,
    pub(crate) op: &'a str,
    pub(crate) args: &'a RawValue,
}

/// The agent's first line: `{"hello":{"proto":1,"agent":"<hash>","release":"25.12.6","caps":{…}}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub proto: u32,
    /// Hash of the agent bundle that runs.
    pub agent: String,
    /// The OpenWrt release, such as `25.12.6`.
    #[serde(default)]
    pub release: Option<String>,
    #[serde(default)]
    pub caps: AgentCaps,
}

/// What the running agent can do.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentCaps {
    /// The agent runs an event loop (ucode-mod-uloop): it serves requests concurrently and can
    /// push events and ticks. Without one, it serves requests one at a time.
    pub events: bool,
    /// The ops it serves.
    pub ops: Vec<String>,
    /// The collect sets it knows. Strings, so a newer agent's sets don't break an older
    /// controller.
    pub sets: Vec<String>,
}

impl AgentCaps {
    pub fn has_set(&self, set: ops::Set) -> bool {
        self.sets.iter().any(|name| name == set.as_str())
    }
}

/// An error answer: `{"code":"timeout","msg":"…"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteError {
    pub code: String,
    pub msg: String,
}

/// Something the agent pushed without being asked.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// `{"ev":"hostapd.assoc","ts":…,"data":{…}}`.
    Event {
        topic: String,
        /// The node's clock, which may be skewed (ARCHITECTURE.md, Failure modes).
        ts: Option<Timestamp>,
        data: Arc<RawValue>,
    },
    /// `{"tick":{…}}`: the results of periodic collection.
    Tick(Arc<RawValue>),
}

/// A line from the agent, classified.
#[derive(Debug)]
pub(crate) enum Message {
    Hello(Hello),
    Answer {
        id: u64,
        result: Result<Box<RawValue>, RemoteError>,
    },
    Event(AgentEvent),
}

/// Every field any line may carry; the line is parsed once and then classified.
#[derive(Deserialize)]
struct Line {
    id: Option<u64>,
    ok: Option<Box<RawValue>>,
    err: Option<RemoteError>,
    ev: Option<String>,
    ts: Option<Timestamp>,
    data: Option<Box<RawValue>>,
    tick: Option<Box<RawValue>>,
    hello: Option<Hello>,
}

impl Message {
    pub(crate) fn parse(line: &[u8]) -> Result<Self, String> {
        // ucode passes the bytes of command output and files through unchanged, so a string may
        // hold invalid UTF-8; such bytes become U+FFFD rather than costing the whole line.
        let line = String::from_utf8_lossy(line);
        let line: Line = serde_json::from_str(&line).map_err(|err| err.to_string())?;
        Ok(match line {
            Line {
                hello: Some(hello), ..
            } => Self::Hello(hello),
            Line {
                id: Some(id),
                err: Some(err),
                ..
            } => Self::Answer {
                id,
                result: Err(err),
            },
            // `"ok":null` deserializes as no field at all: the op returned nothing.
            Line {
                id: Some(id), ok, ..
            } => Self::Answer {
                id,
                result: Ok(ok.unwrap_or_else(null)),
            },
            Line {
                ev: Some(topic),
                ts,
                data,
                ..
            } => Self::Event(AgentEvent::Event {
                topic,
                ts,
                data: data.unwrap_or_else(null).into(),
            }),
            Line {
                tick: Some(tick), ..
            } => Self::Event(AgentEvent::Tick(tick.into())),
            _ => return Err("neither an answer, an event, a tick nor a hello".to_owned()),
        })
    }
}

fn null() -> Box<RawValue> {
    RawValue::NULL.to_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::value::RawValue;
    use uniwrt_core::Timestamp;

    use super::{AgentCaps, AgentEvent, Message, PROTO, Request, SUPPORTED, ops::Set};

    #[test]
    fn requests_serialize_as_one_compact_object() {
        let args = RawValue::from_string(r#"{"sets":["system"]}"#.to_owned()).unwrap();
        let line = serde_json::to_string(&Request {
            id: 7,
            op: "collect",
            args: &args,
        })
        .unwrap();
        assert_eq!(
            line,
            r#"{"id":7,"op":"collect","args":{"sets":["system"]}}"#
        );
    }

    #[test]
    fn every_kind_of_line_is_classified() {
        let parse = |line: &str| Message::parse(line.as_bytes()).unwrap();

        let Message::Hello(hello) = parse(
            r#"{"hello":{"proto":1,"agent":"ab12","release":"25.12.6","caps":{"events":true,"sets":["system"],"later":1}}}"#,
        ) else {
            panic!("hello");
        };
        assert_eq!((hello.proto, hello.agent.as_str()), (1, "ab12"));
        assert_eq!(hello.release.as_deref(), Some("25.12.6"));
        assert!(hello.caps.events);
        assert!(hello.caps.has_set(Set::System));
        assert!(!hello.caps.has_set(Set::Board));
        // An agent without caps serves requests one at a time.
        let Message::Hello(hello) = parse(r#"{"hello":{"proto":1,"agent":"old"}}"#) else {
            panic!("hello without caps");
        };
        assert_eq!(hello.caps, AgentCaps::default());

        let Message::Answer {
            id: 7,
            result: Ok(ok),
        } = parse(r#"{"id":7,"ok":{"a":1}}"#)
        else {
            panic!("ok");
        };
        assert_eq!(ok.get(), r#"{"a":1}"#);
        let Message::Answer {
            id: 8,
            result: Ok(ok),
        } = parse(r#"{"id":8,"ok":null}"#)
        else {
            panic!("null");
        };
        assert_eq!(ok.get(), "null");
        let Message::Answer {
            id: 9,
            result: Err(err),
        } = parse(r#"{"id":9,"err":{"code":"timeout","msg":"batctl did not exit"}}"#)
        else {
            panic!("err");
        };
        assert_eq!(err.code, "timeout");

        let Message::Event(AgentEvent::Event { topic, ts, data }) =
            parse(r#"{"ev":"hostapd.assoc","ts":1761091200123,"data":{"ifname":"phy1-ap0"}}"#)
        else {
            panic!("event");
        };
        assert_eq!(topic, "hostapd.assoc");
        assert_eq!(ts, Some(Timestamp::from_unix_ms(1_761_091_200_123)));
        assert_eq!(data.get(), r#"{"ifname":"phy1-ap0"}"#);

        let Message::Event(AgentEvent::Tick(tick)) = parse(r#"{"tick":{"system":{}}}"#) else {
            panic!("tick");
        };
        assert_eq!(tick.get(), r#"{"system":{}}"#);
    }

    #[test]
    fn invalid_utf8_in_strings_is_replaced_not_fatal() {
        // ucode 25.12 prints a command's raw bytes inside JSON strings.
        let line = b"{\"id\":1,\"ok\":{\"stdout\":\"a\x80\xffb\"}}";
        let Message::Answer {
            id: 1,
            result: Ok(ok),
        } = Message::parse(line).unwrap()
        else {
            panic!("answer");
        };
        assert_eq!(ok.get(), "{\"stdout\":\"a\u{fffd}\u{fffd}b\"}");
    }

    #[test]
    fn malformed_lines_are_refused() {
        assert!(Message::parse(b"{}").is_err());
        assert!(Message::parse(b"not json").is_err());
        assert!(Message::parse(br#"{"hello":{"proto":"one"}}"#).is_err());
    }

    #[test]
    fn this_controller_speaks_v1() {
        assert_eq!(PROTO, 1);
        assert_eq!(SUPPORTED, 1..=1);
    }
}
