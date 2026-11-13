//! The mesh's synchronized cutover on the hwsim lab (features/wifi.md, RF planner: Apply;
//! M6.1 b2, D-114): node a (25.12) and node b (24.10) move their mesh from channel 36 to 149
//! in transactions scheduled for one instant ([`txn::apply_at`]). Nothing moves before it; both
//! reload within a second of each other, and their 802.11s peer link forms again on 149; rolled
//! back, it forms again on 36. Ignored by default: `just test-wifi` runs it after the lab's other
//! node tests, with `UNIWRT_LAB_GW`, `UNIWRT_LAB_AP` and `UNIWRT_LAB_PASSWORD`.

// Test code throughout: clippy.toml allows unwrap in tests, but not in their helpers.
#![allow(clippy::unwrap_used)]

mod openwrt;

use std::{
    env,
    time::{Duration, Instant},
};

use openwrt::{adopt_at, run};
use serde::de::DeserializeOwned;
use uniwrt_node::{
    Error, ExecRequest, Session,
    txn::{self, Command, Footprint, OpenState, Outcome, Reason, TxnId},
};

/// How far ahead the instant is: time for both nodes to install and answer.
const LEAD: Duration = Duration::from_secs(8);
/// How long the mesh may take to form again after a reload.
const MESHED: Duration = Duration::from_secs(90);

async fn exec<T: DeserializeOwned>(session: &Session, command: &Command<T>) -> Result<T, Error> {
    command.parse(session.exec(&command.request).await?)
}

async fn shell_ok(session: &Session, script: &str) -> bool {
    let request = ExecRequest::new(["sh", "-c", script]);
    session.exec(&request).await.unwrap().status.success()
}

/// Logs in to the lab node in `$var` with the footprint installed and nothing open.
async fn lab(var: &str) -> Session {
    let address = env::var(var).unwrap_or_else(|_| panic!("{var}=<host:port>"));
    let password = env::var("UNIWRT_LAB_PASSWORD").expect("UNIWRT_LAB_PASSWORD");
    let (session, _) = adopt_at(&address, &password).await;
    if let Some(open) = exec(&session, &txn::status())
        .await
        .unwrap()
        .and_then(|s| s.open)
    {
        let id = TxnId::new(open.id).unwrap();
        exec(&session, &txn::rollback(&id)).await.unwrap();
    }
    exec(&session, &Footprint::get().install()).await.unwrap();
    session
}

/// The node's mesh point runs `channel` and its peer link is established.
fn meshed_on(channel: u16) -> String {
    format!(
        r#"m=$(iw dev | awk '/Interface/ {{ i = $2 }} /type mesh point/ {{ print i }}')
        [ -n "$m" ] && iw dev "$m" info | grep -q "channel {channel} " &&
        iw dev "$m" station dump | grep -q "mesh plink:.*ESTAB""#
    )
}

async fn wait_meshed(nodes: &[&Session], channel: u16) {
    let start = Instant::now();
    let script = meshed_on(channel);
    loop {
        let mut all = true;
        for node in nodes {
            all &= shell_ok(node, &script).await;
        }
        if all {
            return;
        }
        if start.elapsed() > MESHED {
            let state = "iw dev; for m in $(iw dev | awk '/Interface/ { print $2 }'); do \
                iw dev $m station dump; done; logread | tail -n 30";
            for node in nodes {
                eprintln!("{}", run(node, &["sh", "-c", state]).await);
            }
            panic!("the mesh did not form on channel {channel}");
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// When the tool last logged that `id` reloads as scheduled, in seconds of the day (the
/// containers share the host's clock).
async fn reloaded_at(session: &Session, id: &TxnId) -> u32 {
    let script = format!("logread -e 'transaction {id} reloads as scheduled' | tail -n 1");
    let line = run(session, &["sh", "-c", &script]).await;
    let time = line
        .split_whitespace()
        .nth(3)
        .unwrap_or_else(|| panic!("{line:?}"));
    time.split(':')
        .map(|part| part.parse::<u32>().unwrap())
        .fold(0, |seconds, part| seconds * 60 + part)
}

/// Applies `id` on `node` with the countdown left to `at`.
async fn schedule(
    node: &Session,
    id: &TxnId,
    at: tokio::time::Instant,
) -> Result<txn::Armed, Error> {
    let wait = at.saturating_duration_since(tokio::time::Instant::now());
    exec(node, &txn::apply_at(id, Duration::from_secs(300), wait)).await
}

#[tokio::test]
#[ignore = "needs the hwsim lab: just test-wifi"]
async fn the_mesh_moves_on_every_node_at_one_instant() {
    let (a, b) = (lab("UNIWRT_LAB_GW").await, lab("UNIWRT_LAB_AP").await);
    let nodes = [&a, &b];
    wait_meshed(&nodes, 36).await;
    let id = TxnId::new("lab-cutover").unwrap();
    for node in nodes {
        // Each node's 5 GHz radio, which carries the mesh, is the one on channel 36.
        let wireless = run(node, &["cat", "/etc/config/wireless"]).await;
        assert_eq!(wireless.matches("option channel '36'").count(), 1);
        let moved = wireless.replace("option channel '36'", "option channel '149'");
        exec(node, &txn::stage(&id, []).unwrap()).await.unwrap();
        exec(node, &txn::put(&id, "wireless", moved).unwrap())
            .await
            .unwrap();
        exec(node, &txn::check(&id)).await.unwrap();
    }

    let at = tokio::time::Instant::now() + LEAD;
    let (on_a, on_b) = tokio::join!(schedule(&a, &id, at), schedule(&b, &id, at));
    for armed in [on_a, on_b] {
        let armed = armed.unwrap();
        assert!(armed.wait_ms.is_some_and(|ms| ms > 3000), "{armed:?}");
    }
    // Installed, and nothing moved yet.
    for node in nodes {
        let open = exec(node, &txn::status())
            .await
            .unwrap()
            .unwrap()
            .open
            .unwrap();
        assert_eq!(open.state, OpenState::Reloading);
        assert!(shell_ok(node, &meshed_on(36)).await, "the mesh moved early");
    }

    wait_meshed(&nodes, 149).await;
    assert!(tokio::time::Instant::now() >= at);
    let (ta, tb) = (reloaded_at(&a, &id).await, reloaded_at(&b, &id).await);
    assert!(ta.abs_diff(tb) <= 1, "a reloaded at {ta} s, b at {tb} s");

    for node in nodes {
        exec(node, &txn::rollback(&id)).await.unwrap();
        let last = exec(node, &txn::status())
            .await
            .unwrap()
            .unwrap()
            .last
            .unwrap();
        assert_eq!(
            (last.outcome, last.reason),
            (Outcome::RolledBack, Some(Reason::Requested))
        );
    }
    wait_meshed(&nodes, 36).await;
}
