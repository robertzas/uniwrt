//! The SSH check of the verify phase (features/security.md, Node hardening; D-150). A transaction
//! that rewrites `dropbear` restarts its servers, while the logins it was applied over live on in
//! their own processes, so neither they nor a new login made before the restart show what the
//! new servers let in. Once applied, the node must run its servers as the rendered file asks
//! ([`SshServers`]), then a new login must reach the servers that run; otherwise the apply rolls
//! back, so hardening never locks this controller out.

use std::sync::atomic::{AtomicBool, Ordering};

use tokio::time::{Instant, sleep};
use uniwrt_node::proto::ops::Set;
use uniwrt_render::SshServers;

use super::{run::Live, survey, verify::STOPPED};
use crate::runner::Shared;

const SETS: [Set; 1] = [Set::Services];

/// What the part's `dropbear` file asks of the servers, if it rewrites one.
fn wanted(live: &Live) -> Option<SshServers> {
    let (_, text) = live
        .part
        .files
        .iter()
        .find(|(name, _)| name == "dropbear")?;
    SshServers::of(&uniwrt_uci::parse("dropbear", text).ok()?)
}

/// Checks `live`'s SSH servers until `until`, where its transaction rewrote them.
pub(super) async fn check(
    shared: &Shared,
    live: &mut Live,
    candidates: &[String],
    until: Instant,
    stop: &AtomicBool,
) -> Result<(), String> {
    let Some(want) = wanted(live) else {
        return Ok(());
    };
    let late = || Instant::now() >= until;
    loop {
        if stop.load(Ordering::SeqCst) {
            return Err(STOPPED.to_owned());
        }
        let why = if let Some(remote) = live.remote.as_deref() {
            match survey::collect(shared, remote, &SETS, &SETS).await {
                Ok(facts) => match want.check(facts.services.ssh.as_deref()) {
                    Ok(()) => break,
                    Err(why) => why,
                },
                Err(why) => {
                    live.reconnect(shared, candidates).await;
                    why
                }
            }
        } else {
            live.reconnect(shared, candidates).await;
            "Its login dropped".to_owned()
        };
        if late() {
            return Err(format!("SSH doesn't run as applied: {why}"));
        }
        sleep(shared.timing.poll).await;
    }
    loop {
        if stop.load(Ordering::SeqCst) {
            return Err(STOPPED.to_owned());
        }
        match survey::login(shared, &live.node, &live.host).await {
            Ok((_, remote)) => {
                live.replace_remote(remote).await;
                return Ok(());
            }
            Err(why) if late() => {
                return Err(format!("No new SSH login since SSH changed: {why}"));
            }
            Err(_) => sleep(shared.timing.poll).await,
        }
    }
}
