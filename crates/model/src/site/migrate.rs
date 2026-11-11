//! Schema migrations: forward-only steps that bring a stored document to [`SCHEMA_VERSION`].
//! Revisions keep the JSON they were written with; reading one upgrades it.
//!
//! To change the schema: raise [`SCHEMA_VERSION`], append a step to [`STEPS`] that rewrites the
//! JSON of the previous version, and add `fixtures/site/v<new>.json` to the tests' `FIXTURES`,
//! which upgrade the fixture of every version.

use serde_json::{Value, json};

use super::{SCHEMA_VERSION, SiteConfig};

/// Rewrites the JSON of one version into the next: `STEPS[0]` turns v1 into v2, and so on.
type Step = fn(&mut Value) -> Result<(), String>;

const STEPS: &[Step] = &[v1_to_v2, v2_to_v3, v3_to_v4, v4_to_v5, v5_to_v6, v6_to_v7];

/// v2 (M4.1, D-075): the WAN's IPv6 becomes a mode (`ipv6: true` is DHCPv6 with delegation),
/// and it gains the ISP preset, a PCP and an MTU; DHCP gains its client options and PPPoE a
/// service name.
fn v1_to_v2(value: &mut Value) -> Result<(), String> {
    let wan = value
        .get_mut("wan")
        .and_then(Value::as_object_mut)
        .ok_or("the document has no WAN")?;
    let ipv6 = match wan.get("ipv6") {
        Some(Value::Bool(true)) => json!({ "mode": "dhcpv6", "reqprefix": null }),
        Some(Value::Bool(false)) => json!({ "mode": "off" }),
        _ => return Err("the WAN's ipv6 is not a boolean".to_owned()),
    };
    wan.insert("ipv6".to_owned(), ipv6);
    for field in ["pcp", "mtu", "preset"] {
        wan.insert(field.to_owned(), Value::Null);
    }
    let proto = wan
        .get_mut("proto")
        .and_then(Value::as_object_mut)
        .ok_or("the WAN has no proto")?;
    match proto.get("kind").and_then(Value::as_str) {
        Some("dhcp") => {
            for field in ["vendor_class", "client_id", "hostname"] {
                proto.insert(field.to_owned(), Value::Null);
            }
            proto.insert("send_options".to_owned(), json!([]));
        }
        Some("pppoe") => {
            proto.insert("service".to_owned(), Value::Null);
        }
        _ => {}
    }
    Ok(())
}

/// v3 (M4.1b, D-076): the WAN gains SQM and failover, both off, and every node that manages
/// `network` manages `sqm` and `mwan3` too (render writes them only on the gateway, only where
/// they are on or the node has them already).
fn v2_to_v3(value: &mut Value) -> Result<(), String> {
    let wan = value
        .get_mut("wan")
        .and_then(Value::as_object_mut)
        .ok_or("the document has no WAN")?;
    wan.insert("sqm".to_owned(), Value::Null);
    wan.insert("failover".to_owned(), Value::Null);
    let nodes = value
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or("the document has no nodes")?;
    for node in nodes {
        let Some(managed) = node.get_mut("managed").and_then(Value::as_array_mut) else {
            return Err("a node has no managed packages".to_owned());
        };
        if managed.iter().any(|package| package == "network") {
            for package in ["sqm", "mwan3"] {
                if !managed.iter().any(|p| p == package) {
                    managed.push(json!(package));
                }
            }
        }
    }
    Ok(())
}

/// v4 (M4.2a, D-077): networks gain how clients get IPv6 (SLAAC with stateless DHCPv6, as
/// rendered before) and their pools DHCP options (none); nodes gain port profiles (none: every
/// port a trunk).
fn v3_to_v4(value: &mut Value) -> Result<(), String> {
    let networks = value
        .get_mut("networks")
        .and_then(Value::as_array_mut)
        .ok_or("the document has no networks")?;
    for network in networks {
        let network = network
            .as_object_mut()
            .ok_or("a network is not an object")?;
        network.insert("ipv6_clients".to_owned(), json!("slaac_stateless"));
        if let Some(dhcp) = network
            .get_mut("ipv4")
            .and_then(|ipv4| ipv4.get_mut("dhcp"))
            .and_then(Value::as_object_mut)
        {
            dhcp.insert(
                "options".to_owned(),
                json!({ "dns": [], "ntp": [], "domain": null, "boot": null, "custom": [] }),
            );
        }
    }
    let nodes = value
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or("the document has no nodes")?;
    for node in nodes {
        let node = node.as_object_mut().ok_or("a node is not an object")?;
        node.insert("ports".to_owned(), json!([]));
    }
    Ok(())
}

/// v5 (M4.2b, D-079): the firewall gains port forwards and a DMZ host (none) and UPnP, off as
/// rendered before, for the default network when turned on; nodes that manage the firewall
/// manage `upnpd` too (render writes it only while UPnP is on or the node has it already).
fn v4_to_v5(value: &mut Value) -> Result<(), String> {
    let lan = value
        .get("networks")
        .and_then(Value::as_array)
        .and_then(|networks| {
            networks
                .iter()
                .find(|n| n.get("purpose").and_then(Value::as_str) == Some("default"))
        })
        .and_then(|n| n.get("id").cloned())
        .unwrap_or_else(|| json!("lan"));
    let firewall = value
        .get_mut("firewall")
        .and_then(Value::as_object_mut)
        .ok_or("the document has no firewall")?;
    firewall.insert("forwards".to_owned(), json!([]));
    firewall.insert("dmz".to_owned(), Value::Null);
    firewall.insert(
        "upnp".to_owned(),
        json!({ "enabled": false, "networks": [lan] }),
    );
    let nodes = value
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or("the document has no nodes")?;
    for node in nodes {
        let Some(managed) = node.get_mut("managed").and_then(Value::as_array_mut) else {
            return Err("a node has no managed packages".to_owned());
        };
        if managed.iter().any(|p| p == "firewall") && !managed.iter().any(|p| p == "upnpd") {
            managed.push(json!("upnpd"));
        }
    }
    Ok(())
}

/// v6 (M5.1, D-083): WLANs gain a client limit and a schedule, both unset.
fn v5_to_v6(value: &mut Value) -> Result<(), String> {
    let wlans = value
        .get_mut("wlans")
        .and_then(Value::as_array_mut)
        .ok_or("the document has no WLANs")?;
    for wlan in wlans {
        let wlan = wlan.as_object_mut().ok_or("a WLAN is not an object")?;
        wlan.insert("max_clients".to_owned(), Value::Null);
        wlan.insert("schedule".to_owned(), Value::Null);
    }
    Ok(())
}

/// v7 (M5.2, D-085): WLANs gain roaming: 802.11r as the security mode suggests, 11k/v on.
fn v6_to_v7(value: &mut Value) -> Result<(), String> {
    let wlans = value
        .get_mut("wlans")
        .and_then(Value::as_array_mut)
        .ok_or("the document has no WLANs")?;
    for wlan in wlans {
        let wlan = wlan.as_object_mut().ok_or("a WLAN is not an object")?;
        wlan.insert("roaming".to_owned(), json!({ "fast": null, "hints": true }));
    }
    Ok(())
}

const _: () = assert!(STEPS.len() + 1 == SCHEMA_VERSION as usize);

/// Why a stored document cannot be read.
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("the document has no schema version")]
    Missing,
    #[error("the document's schema {found} is newer than this uniwrt knows ({SCHEMA_VERSION})")]
    TooNew { found: u64 },
    #[error("upgrading the document from schema {from} failed: {reason}")]
    Step { from: u64, reason: String },
    #[error("the document does not match schema {SCHEMA_VERSION}")]
    Invalid(#[source] serde_json::Error),
}

/// The document `value`, upgraded to the current schema.
pub fn upgrade(mut value: Value) -> Result<SiteConfig, SchemaError> {
    let found = value
        .get("schema")
        .and_then(Value::as_u64)
        .filter(|&found| found >= 1)
        .ok_or(SchemaError::Missing)?;
    if found > u64::from(SCHEMA_VERSION) {
        return Err(SchemaError::TooNew { found });
    }
    for (from, step) in (found..).zip(STEPS.iter().skip(usize::try_from(found - 1).unwrap_or(0))) {
        step(&mut value).map_err(|reason| SchemaError::Step { from, reason })?;
        if let Some(schema) = value.get_mut("schema") {
            *schema = (from + 1).into();
        }
    }
    serde_json::from_value(value).map_err(SchemaError::Invalid)
}
