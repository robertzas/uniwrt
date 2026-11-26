//! The schema steps from v20 on, kept apart so the module stays small.

use serde_json::{Map, Value, json};

/// v20 (M9.0b1, D-146): the upstream DNS becomes ordered layers (encrypted resolvers, plain
/// servers, the ISP's) with a strict switch: the ISP's stays the ISP's alone, custom servers stay
/// alone (the ISP's off), and DoH stays strict (DoH alone, as dnsmasq asked it before) with each
/// resolver tagged `doh`; nodes that manage `dhcp` manage `stubby` too (render writes it only
/// on the gateway, while DoT is in use or the package is held).
pub(super) fn v19_to_v20(value: &mut Value) -> Result<(), String> {
    let services = value
        .get_mut("services")
        .and_then(Value::as_object_mut)
        .ok_or("the document has no services")?;
    let old = services
        .get("upstream")
        .and_then(Value::as_object)
        .ok_or("the services have no upstream")?;
    let list = |key: &str| -> Result<Vec<Value>, String> {
        old.get(key)
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| format!("the upstream has no {key}"))
    };
    let upstream = match old.get("kind").and_then(Value::as_str) {
        Some("isp") => json!({ "encrypted": [], "servers": [], "isp": true, "strict": false }),
        Some("custom") => {
            json!({ "encrypted": [], "servers": list("servers")?, "isp": false, "strict": false })
        }
        Some("doh") => {
            let mut encrypted = Vec::new();
            for resolver in list("resolvers")? {
                let Value::Object(fields) = resolver else {
                    return Err("a DoH resolver is not an object".to_owned());
                };
                let mut tagged = Map::from_iter([("protocol".to_owned(), json!("doh"))]);
                tagged.extend(fields);
                encrypted.push(Value::Object(tagged));
            }
            json!({ "encrypted": encrypted, "servers": [], "isp": false, "strict": true })
        }
        _ => return Err("the upstream's kind is unknown".to_owned()),
    };
    services.insert("upstream".to_owned(), upstream);
    let nodes = value
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or("the document has no nodes")?;
    for node in nodes {
        let Some(managed) = node.get_mut("managed").and_then(Value::as_array_mut) else {
            return Err("a node has no managed packages".to_owned());
        };
        if managed.iter().any(|p| p == "dhcp") && !managed.iter().any(|p| p == "stubby") {
            managed.push(json!("stubby"));
        }
    }
    Ok(())
}

/// v21 (M9.3a2b2, D-150): nodes gain SSH hardening, off; none manages `dropbear` yet (render
/// writes it only for nodes that opt in).
pub(super) fn v20_to_v21(value: &mut Value) -> Result<(), String> {
    let nodes = value
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or("the document has no nodes")?;
    for node in nodes {
        node.as_object_mut()
            .ok_or("a node is not an object")?
            .insert(
                "ssh".to_owned(),
                json!({ "key_only": false, "management_only": false }),
            );
    }
    Ok(())
}
