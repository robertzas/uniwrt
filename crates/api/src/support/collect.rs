//! What the support bundle holds (D-144). From the controller: `README.txt`; `controller/`
//! `about.json` (versions, when), `site.json` (the latest revision as a site export, without
//! secrets: they stay in the vault), `nodes.json` (the inventory with each node's status),
//! `events.json` and `logs.txt` (the last 24 h stored, at most [`EVENTS_MAX`] and
//! [`LOGS_MAX`]), `wan-monitor.json` and `log.txt` (the controller's own log tail) where
//! they run. From each node, under `nodes/<id>-<name>/`: `facts.json` (the latest facts) and,
//! while it is online, `uci.json` (its live UCI, masked as the explorer masks it) and
//! `snapshot/` (`uniwrt_node::support`: board, addresses, routes, Wi-Fi status, `dmesg`,
//! `logread`…, JSON masked). What couldn't be read is listed in `problems.txt`. The nodes are
//! read at once, each bounded by its commands' timeouts.

use std::{fmt::Write as _, time::Duration};

use futures_util::future::join_all;
use serde::Serialize;
use serde_json::json;
use uniwrt_core::Timestamp;
use uniwrt_model::{
    events::EventFilter,
    log::{LogFilter, LogRecord},
    node::Node,
    site::export::SiteFile,
    status::NodeState,
};
use uniwrt_node::support;
use uniwrt_store::{EventRepo as _, LogRepo as _, NodeRepo as _, RevisionRepo as _};

use crate::{Api, ApiResult};

pub(super) const EVENTS_MAX: usize = 5_000;
pub(super) const LOGS_MAX: usize = 20_000;
const SPAN: Duration = Duration::from_hours(24);

type Files = Vec<(String, Vec<u8>)>;

fn pretty(value: &impl Serialize) -> Vec<u8> {
    let mut text = serde_json::to_vec_pretty(value).unwrap_or_default();
    text.push(b'\n');
    text
}

/// A node's directory: its id and its name in letters, digits and dashes.
fn dir(node: &Node) -> String {
    let name: String = node
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(24)
        .collect();
    format!("nodes/{}-{}", node.id, name.trim_matches('-'))
}

pub(super) async fn files(api: &Api, now: Timestamp) -> ApiResult<Files> {
    let store = api.store();
    let from = now.saturating_sub(SPAN);
    let nodes = store.nodes().await?;
    let head = store.head_revision().await?;
    let events = EventFilter {
        from: Some(from),
        ..EventFilter::default()
    };
    let mut events = store.events(events, None, EVENTS_MAX).await?;
    events.reverse();
    let logs = LogFilter {
        from: Some(from),
        ..LogFilter::default()
    };
    let mut logs = store.logs(logs, None, LOGS_MAX).await?;
    logs.reverse();
    let build = api.build();
    let mut files: Files = Vec::new();
    let mut problems = Vec::new();
    files.push((
        "README.txt".to_owned(),
        readme(now, build.version).into_bytes(),
    ));
    let about = json!({
        "version": build.version,
        "commit": build.commit,
        "built_at": build.built_at,
        "generated_at": now,
        "span_from": from,
    });
    files.push(("controller/about.json".to_owned(), pretty(&about)));
    if let Some(head) = head {
        match SiteFile::new(head.meta.id, &head.config, &nodes, now, build.version, None) {
            Ok(file) => files.push(("controller/site.json".to_owned(), pretty(&file))),
            Err(err) => problems.push(format!("site: {err}")),
        }
    }
    let inventory: Vec<_> = nodes
        .iter()
        .map(|node| {
            let status = api.fleet().node(node.id).map(|view| view.status);
            json!({ "node": node, "status": status })
        })
        .collect();
    files.push(("controller/nodes.json".to_owned(), pretty(&inventory)));
    files.push(("controller/events.json".to_owned(), pretty(&events)));
    files.push((
        "controller/logs.txt".to_owned(),
        log_lines(&logs, &nodes).into_bytes(),
    ));
    if let Ok(view) = crate::wan_monitor::view(api) {
        files.push(("controller/wan-monitor.json".to_owned(), pretty(&view)));
    }
    if let Some(tail) = api.log_tail() {
        files.push(("controller/log.txt".to_owned(), tail.text().into_bytes()));
    }
    for (node_files, node_problems) in join_all(nodes.iter().map(|node| node_part(api, node))).await
    {
        files.extend(node_files);
        problems.extend(node_problems);
    }
    if !problems.is_empty() {
        let text = problems.join("\n") + "\n";
        files.push(("problems.txt".to_owned(), text.into_bytes()));
    }
    Ok(files)
}

fn readme(now: Timestamp, version: &str) -> String {
    format!(
        "uniwrt support bundle, made {now} UTC by uniwrt {version}.

controller/  what the controller stores: its versions, the site (secrets stay in its vault),
             the nodes and their status, the last 24 h of events and logs, the WAN monitor,
             and its own latest log lines.
nodes/       per node: its latest facts and, if it was online, its UCI configuration and a
             snapshot (board, addresses, routes, Wi-Fi, dmesg, logread). Options whose names
             say they hold a secret are masked (********).
problems.txt what couldn't be read, if anything.

Look it over before you share it: logs may name your devices and networks.
"
    )
}

fn log_lines(logs: &[LogRecord], nodes: &[Node]) -> String {
    let mut text = String::new();
    for record in logs {
        let source = record
            .node
            .and_then(|id| nodes.iter().find(|node| node.id == id))
            .map(|node| node.name.clone())
            .or_else(|| record.host.clone())
            .unwrap_or_else(|| "?".to_owned());
        let entry = &record.entry;
        let program = entry.program.as_deref().unwrap_or("-");
        let pid = entry.pid.map_or_else(String::new, |pid| format!("[{pid}]"));
        let _ = writeln!(
            text,
            "{} {source} {:?} {program}{pid}: {}",
            entry.time, entry.severity, entry.message
        );
    }
    text
}

/// A node's files, and what couldn't be read of it.
async fn node_part(api: &Api, node: &Node) -> (Files, Vec<String>) {
    let dir = dir(node);
    let mut files: Files = Vec::new();
    let mut problems = Vec::new();
    let view = api.fleet().node(node.id);
    if let Some(facts) = view.as_ref().and_then(|view| view.facts.as_ref()) {
        files.push((format!("{dir}/facts.json"), pretty(facts.as_ref())));
    }
    let online = view.is_some_and(|view| view.status.state != NodeState::Offline);
    if !online {
        problems.push(format!(
            "{}: offline, so nothing was read from it",
            node.name
        ));
        return (files, problems);
    }
    let (uci, snapshot) = tokio::join!(
        crate::uci::read(api, node),
        api.fleet().exec(node.id, support::snapshot())
    );
    match uci {
        Ok(uci) => files.push((format!("{dir}/uci.json"), pretty(&uci))),
        Err(err) => problems.push(format!("{}: UCI: {err}", node.name)),
    }
    match snapshot {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for (name, text) in support::sections(&stdout) {
                files.push((format!("{dir}/snapshot/{name}"), text.into_bytes()));
            }
        }
        Err(err) => problems.push(format!("{}: snapshot: {err}", node.name)),
    }
    (files, problems)
}
