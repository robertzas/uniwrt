//! usteer's view of a client, read through the `ubus` op (features/wifi.md, Steering &
//! balancing; D-093): the controller asks the client's node's usteer where else it is heard,
//! since usteer learns every access point's probes from the other nodes' usteer.

use std::{collections::BTreeMap, net::IpAddr};

use serde::Deserialize;
use serde_json::{Map, Value, value::RawValue};
use uniwrt_core::MacAddr;

use super::Ubus;
use crate::events::usteer::{dbm, node_name};

/// `ubus call usteer get_client_info '{"address": …}'`. usteer answers `not found` (ubus status
/// 4) for a client it never heard.
pub fn client_info(mac: MacAddr) -> Ubus {
    let mut data = Map::new();
    data.insert("address".to_owned(), Value::String(mac.to_string()));
    Ubus {
        path: "usteer".to_owned(),
        method: "get_client_info".to_owned(),
        data,
    }
}

/// An access point usteer heard the client at, with the client's signal there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeardAt {
    /// The address of the node's usteer; `None` for the node asked.
    pub host: Option<IpAddr>,
    pub iface: String,
    pub signal_dbm: i16,
}

#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    nodes: BTreeMap<String, Entry>,
}

#[derive(Deserialize)]
struct Entry {
    signal: Option<i64>,
}

/// The access points in a `get_client_info` reply (`{"nodes": {<usteer name>: {"signal": …,
/// "connected": …}}}`), leaving out those without a signal. `connected` isn't read: usteer
/// sends its three states as a u8, which the agent's ucode reads as a boolean, so a client that
/// left an access point still reads as connected there.
pub fn heard(reply: &RawValue) -> Result<Vec<HeardAt>, serde_json::Error> {
    let reply: Reply = serde_json::from_str(reply.get())?;
    Ok(reply
        .nodes
        .iter()
        .filter_map(|(name, entry)| {
            let signal_dbm = entry.signal.and_then(dbm).filter(|&s| s < 0)?;
            let (host, iface) = node_name(name);
            Some(HeardAt {
                host,
                iface: iface.to_owned(),
                signal_dbm,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_request_names_the_client() {
        let request = client_info(MacAddr::new([2, 0, 0, 0, 4, 0]));
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({"path": "usteer", "method": "get_client_info",
                "data": {"address": "02:00:00:00:04:00"}})
        );
    }

    #[test]
    fn the_reply_places_each_access_point_with_its_signal() {
        let stats = json!({"probe": {"requests": 3, "blocked_cur": 0, "blocked_total": 0}});
        let reply = json!({"2ghz": true, "5ghz": true, "nodes": {
            "hostapd.phy1-ap0": {"connected": true, "signal": -78, "stats": stats},
            // A signal as an unsigned u32, and another node's access point.
            "172.17.0.3#hostapd.phy2-ap0": {"connected": false, "signal": 4_294_967_235_u64, "stats": stats},
            // Never heard: no signal.
            "172.17.0.4#hostapd.phy1-ap0": {"connected": false, "signal": 0, "stats": stats},
        }});
        let raw = serde_json::value::to_raw_value(&reply).unwrap();
        assert_eq!(
            heard(&raw).unwrap(),
            vec![
                HeardAt {
                    host: Some("172.17.0.3".parse().unwrap()),
                    iface: "phy2-ap0".to_owned(),
                    signal_dbm: -61,
                },
                HeardAt {
                    host: None,
                    iface: "phy1-ap0".to_owned(),
                    signal_dbm: -78,
                },
            ]
        );
    }
}
