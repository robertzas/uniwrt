//! Clients (features/observability.md, Clients): what the controller keeps about each device
//! that connects through the nodes, by MAC address ([`ClientRecord`]), and where it was
//! connected and when ([`ClientSession`]). What a client looks like now is `uniwrt_clients`'s.

use serde::{Deserialize, Serialize};
use uniwrt_core::{MacAddr, NodeId, Timestamp};

use crate::wifi::Band;

/// The longest alias, in characters.
pub const ALIAS_MAX: usize = 64;

/// What the controller keeps about one client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ClientRecord {
    pub mac: MacAddr,
    /// The name a user gave it.
    pub alias: Option<String>,
    /// The name it last asked the DHCP server for.
    pub hostname: Option<String>,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
    /// When a user blocked it: the controller keeps it off every managed access point until
    /// unblocked (features/wifi.md, Client actions).
    pub blocked: Option<Timestamp>,
}

/// One stay of a client at one place: an access point's interface, or a node's port (or the
/// switch behind it). Sessions of a client don't overlap; `start` identifies one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ClientSession {
    pub mac: MacAddr,
    /// The node it was connected to; `None` behind a mesh peer that is not adopted.
    pub node: Option<NodeId>,
    /// The access point's interface (`phy1-ap0`) or the port (`lan2`), when known.
    pub port: Option<String>,
    pub medium: ClientMedium,
    pub band: Option<Band>,
    pub ssid: Option<String>,
    pub start: Timestamp,
    /// `None` while it lasts.
    pub end: Option<Timestamp>,
    pub ended_by: Option<SessionEnd>,
}

impl ClientSession {
    /// The same place: the node and the port or interface.
    pub fn same_place(&self, other: &Self) -> bool {
        (self.node, &self.port, self.medium) == (other.node, &other.port, other.medium)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ClientMedium {
    Wifi,
    Wired,
}

/// How a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SessionEnd {
    /// The client went away (or the node it was at stopped reporting).
    Left,
    /// The client moved to another access point or port.
    Roamed,
}

/// Why an alias is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AliasError {
    TooLong,
    ControlCharacter,
}

impl std::fmt::Display for AliasError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLong => write!(f, "An alias is at most {ALIAS_MAX} characters long"),
            Self::ControlCharacter => f.write_str("An alias has no control characters"),
        }
    }
}

impl std::error::Error for AliasError {}

/// A user's alias for a client, trimmed; `None` (no alias) when it is blank.
pub fn alias(text: &str) -> Result<Option<String>, AliasError> {
    let text = text.trim();
    if text.chars().count() > ALIAS_MAX {
        return Err(AliasError::TooLong);
    }
    if text.chars().any(char::is_control) {
        return Err(AliasError::ControlCharacter);
    }
    Ok((!text.is_empty()).then(|| text.to_owned()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uniwrt_core::{NodeId, Timestamp};

    use super::{AliasError, ClientMedium, ClientSession, SessionEnd, alias};
    use crate::wifi::Band;

    #[test]
    fn aliases_are_trimmed_and_checked() {
        assert_eq!(
            alias("  Kitchen speaker "),
            Ok(Some("Kitchen speaker".to_owned()))
        );
        assert_eq!(alias("   "), Ok(None));
        assert_eq!(alias(&"é".repeat(64)), Ok(Some("é".repeat(64))));
        assert_eq!(alias(&"x".repeat(65)), Err(AliasError::TooLong));
        assert_eq!(alias("a\u{7}b"), Err(AliasError::ControlCharacter));
    }

    #[test]
    fn sessions_serialize_with_snake_case_names() {
        let session = ClientSession {
            mac: "02:00:00:00:04:00".parse().unwrap(),
            node: Some(NodeId::new(2)),
            port: Some("phy1-ap0".to_owned()),
            medium: ClientMedium::Wifi,
            band: Some(Band::G5),
            ssid: Some("home".to_owned()),
            start: Timestamp::from_unix_ms(1_000),
            end: Some(Timestamp::from_unix_ms(2_000)),
            ended_by: Some(SessionEnd::Roamed),
        };
        let value = serde_json::to_value(&session).unwrap();
        assert_eq!(value["medium"], json!("wifi"));
        assert_eq!(value["ended_by"], json!("roamed"));
        assert_eq!(
            serde_json::from_value::<ClientSession>(value).unwrap(),
            session
        );
        let wired = ClientSession {
            medium: ClientMedium::Wired,
            ..session.clone()
        };
        assert!(!session.same_place(&wired));
        assert!(session.same_place(&ClientSession {
            start: Timestamp::from_unix_ms(5_000),
            ..session.clone()
        }));
    }
}
