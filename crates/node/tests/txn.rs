//! Transactions on real OpenWrt (TESTING.md, Agent; D-069): the footprint installs and goes,
//! and a transaction that replaces `system` and stops dnsmasq is confirmed, rolled back on
//! request, at its deadline or at boot, or refused before it changes anything. Ignored by
//! default: `just test-agent` runs these one at a time after the agent's tests, the boot test
//! last (it restarts the container `UNIWRT_NODE_CONTAINER`).

#![allow(clippy::unwrap_used)]

mod openwrt;

use std::{
    env,
    process::Command as Process,
    time::{Duration, Instant},
};

use openwrt::{adopt, run};
use serde::de::DeserializeOwned;
use uniwrt_node::{
    Error, Session,
    txn::{self, Command, Footprint, OpenState, Outcome, Reason, Status, TxnId, code},
};

async fn exec<T: DeserializeOwned>(session: &Session, command: &Command<T>) -> Result<T, Error> {
    command.parse(session.exec(&command.request).await?)
}

fn txn_code<T: std::fmt::Debug>(result: Result<T, Error>) -> String {
    match result {
        Err(Error::Txn { code, .. }) => code,
        other => panic!("expected an error of the tool, got {other:?}"),
    }
}

async fn status(session: &Session) -> Status {
    exec(session, &txn::status())
        .await
        .unwrap()
        .expect("the footprint is installed")
}

/// Logs in with the footprint installed and nothing open.
async fn managed() -> Session {
    let (session, _) = adopt().await;
    let start = Instant::now();
    while let Some(open) = exec(&session, &txn::status())
        .await
        .unwrap()
        .and_then(|s| s.open)
    {
        if open.state == OpenState::Applied {
            let id = TxnId::new(open.id.clone()).unwrap();
            exec(&session, &txn::rollback(&id)).await.unwrap();
        }
        assert!(start.elapsed() < Duration::from_secs(30), "still {open:?}");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    exec(&session, &Footprint::get().install()).await.unwrap();
    session
}

async fn wait_state(session: &Session, want: OpenState) {
    let start = Instant::now();
    loop {
        let open = status(session).await.open;
        if open.as_ref().is_some_and(|open| open.state == want) {
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "still {open:?}");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn wait_until(session: &Session, what: &str, script: &str) {
    let start = Instant::now();
    while !shell_ok(session, script).await {
        assert!(start.elapsed() < Duration::from_secs(15), "{what}");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn shell_ok(session: &Session, script: &str) -> bool {
    let request = uniwrt_node::ExecRequest::new(["sh", "-c", script]);
    session.exec(&request).await.unwrap().status.success()
}

/// Enabled, and procd has its instance (which may not stay up without a network).
const DNSMASQ_ON: &str = r#"/etc/init.d/dnsmasq enabled && ubus call service list '{"name":"dnsmasq"}' | grep -q instances"#;
const DNSMASQ_OFF: &str = r#"! /etc/init.d/dnsmasq enabled && ! ubus call service list '{"name":"dnsmasq"}' | grep -q instances"#;

/// Stages and applies transaction `id`: `system` with a section marking it, dnsmasq off.
/// Returns `system` as it was.
async fn begin(session: &Session, id: &TxnId, timeout: Duration) -> String {
    let system = run(session, &["cat", "/etc/config/system"]).await;
    let marked = format!("{system}\nconfig uniwrt_test 'probe'\n\toption mark '{id}'\n");
    exec(session, &txn::stage(id, [("dnsmasq", false)]).unwrap())
        .await
        .unwrap();
    exec(session, &txn::put(id, "system", marked).unwrap())
        .await
        .unwrap();
    let checked = exec(session, &txn::check(id)).await.unwrap();
    assert_eq!(checked.packages, ["system"]);
    let armed = exec(session, &txn::apply(id, timeout)).await.unwrap();
    assert_eq!(armed.timeout_s, timeout.as_secs());
    system
}

async fn mark(session: &Session) -> String {
    let request = uniwrt_node::ExecRequest::new(["uci", "-q", "get", "system.probe.mark"]);
    String::from_utf8(session.exec(&request).await.unwrap().stdout)
        .unwrap()
        .trim()
        .to_owned()
}

/// What rolled back transaction `id`, once it did.
fn rolled_back(status: &Status, id: &TxnId) -> Option<Reason> {
    let last = status.last.as_ref()?;
    (last.id == id.as_str() && last.outcome == Outcome::RolledBack)
        .then_some(last.reason)
        .flatten()
}

#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn the_footprint_installs_by_version_and_uninstalls() {
    let (session, _) = adopt().await;
    exec(&session, &txn::uninstall()).await.unwrap();
    assert_eq!(exec(&session, &txn::status()).await.unwrap(), None);

    exec(&session, &Footprint::get().install()).await.unwrap();
    let installed = status(&session).await;
    assert_eq!(installed.version, Footprint::get().version());
    assert_eq!(installed.open, None);
    assert!(shell_ok(&session, "/etc/init.d/uniwrt-guard enabled").await);
    let keep = run(&session, &["cat", "/lib/upgrade/keep.d/uniwrt"]).await;
    assert_eq!(keep, "/etc/uniwrt/\n/etc/init.d/uniwrt-guard\n");
    // Installing again is harmless.
    exec(&session, &Footprint::get().install()).await.unwrap();

    exec(&session, &txn::uninstall()).await.unwrap();
    assert_eq!(exec(&session, &txn::status()).await.unwrap(), None);
    assert!(!shell_ok(&session, "[ -e /etc/uniwrt ] || [ -e /etc/init.d/uniwrt-guard ] || [ -e /lib/upgrade/keep.d/uniwrt ]").await);
}

#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn a_confirmed_transaction_keeps_its_packages_and_services() {
    let session = managed().await;
    let id = TxnId::new("confirmed").unwrap();
    let system = begin(&session, &id, Duration::from_secs(300)).await;
    // One transaction at a time; and confirming waits for the reload.
    let other = TxnId::new("other").unwrap();
    assert_eq!(
        txn_code(exec(&session, &txn::stage(&other, []).unwrap()).await),
        code::BUSY
    );
    wait_state(&session, OpenState::Applied).await;
    let open = status(&session).await.open.unwrap();
    assert!(
        open.remaining_s > 200 && open.remaining_s <= 300,
        "{open:?}"
    );

    exec(&session, &txn::confirm(&id)).await.unwrap();
    let after = status(&session).await;
    assert_eq!(after.open, None);
    let last = after.last.unwrap();
    assert_eq!(
        (last.id.as_str(), last.outcome),
        ("confirmed", Outcome::Confirmed)
    );
    assert_eq!(mark(&session).await, "confirmed");
    assert!(shell_ok(&session, DNSMASQ_OFF).await);
    // The guard runs nothing once the transaction is confirmed, and the journal is gone.
    assert!(!shell_ok(&session, "pgrep -f '[t]xn wait'").await);
    assert!(!shell_ok(&session, "[ -e /etc/uniwrt/journal/confirmed ]").await);
    assert_eq!(
        txn_code(exec(&session, &txn::confirm(&id)).await),
        code::EXPIRED
    );

    // Undone by another transaction, which starts dnsmasq again.
    let undo = TxnId::new("undo").unwrap();
    exec(&session, &txn::stage(&undo, [("dnsmasq", true)]).unwrap())
        .await
        .unwrap();
    exec(
        &session,
        &txn::put(&undo, "system", system.clone()).unwrap(),
    )
    .await
    .unwrap();
    exec(&session, &txn::check(&undo)).await.unwrap();
    exec(&session, &txn::apply(&undo, Duration::from_secs(60)))
        .await
        .unwrap();
    wait_state(&session, OpenState::Applied).await;
    exec(&session, &txn::confirm(&undo)).await.unwrap();
    assert_eq!(run(&session, &["cat", "/etc/config/system"]).await, system);
    wait_until(&session, "dnsmasq runs again", DNSMASQ_ON).await;
}

#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn a_rollback_restores_the_snapshot_and_the_services() {
    let session = managed().await;
    let id = TxnId::new("requested").unwrap();
    let system = begin(&session, &id, Duration::from_secs(300)).await;
    wait_state(&session, OpenState::Applied).await;
    assert_eq!(mark(&session).await, "requested");
    assert!(shell_ok(&session, DNSMASQ_OFF).await);

    exec(&session, &txn::rollback(&id)).await.unwrap();
    let after = status(&session).await;
    assert_eq!(after.open, None);
    assert_eq!(rolled_back(&after, &id), Some(Reason::Requested));
    assert_eq!(run(&session, &["cat", "/etc/config/system"]).await, system);
    wait_until(&session, "dnsmasq runs again", DNSMASQ_ON).await;
    assert_eq!(
        txn_code(exec(&session, &txn::rollback(&id)).await),
        code::EXPIRED
    );
}

#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn an_unconfirmed_transaction_rolls_back_at_its_deadline() {
    let session = managed().await;
    let id = TxnId::new("deadline").unwrap();
    let system = begin(&session, &id, txn::MIN_TIMEOUT).await;
    wait_state(&session, OpenState::Applied).await;
    // Extending moves the deadline.
    let armed = exec(&session, &txn::extend(&id, Duration::from_secs(40)))
        .await
        .unwrap();
    assert_eq!(armed.timeout_s, 40);
    // The timer survives being killed: procd starts it again.
    run(
        &session,
        &["sh", "-c", "kill $(pgrep -f '[t]xn wait') || true"],
    )
    .await;
    let start = Instant::now();
    let reason = loop {
        if let Some(reason) = rolled_back(&status(&session).await, &id) {
            break reason;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "not rolled back");
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    assert_eq!(reason, Reason::Timeout);
    assert!(
        start.elapsed() >= Duration::from_secs(30),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(run(&session, &["cat", "/etc/config/system"]).await, system);
    wait_until(&session, "dnsmasq runs again", DNSMASQ_ON).await;
    assert_eq!(
        txn_code(exec(&session, &txn::confirm(&id)).await),
        code::EXPIRED
    );
}

#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn invalid_transactions_are_refused_before_anything_changes() {
    let session = managed().await;
    let system = run(&session, &["cat", "/etc/config/system"]).await;
    let id = TxnId::new("invalid").unwrap();
    exec(&session, &txn::stage(&id, []).unwrap()).await.unwrap();
    assert_eq!(
        txn_code(exec(&session, &txn::check(&id)).await),
        code::BAD_REQUEST
    );
    exec(
        &session,
        &txn::put(&id, "system", "config a b c d\n").unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        txn_code(exec(&session, &txn::check(&id)).await),
        code::INVALID
    );
    let apply = exec(&session, &txn::apply(&id, Duration::from_secs(60))).await;
    assert_eq!(txn_code(apply), code::BAD_REQUEST);
    exec(
        &session,
        &txn::stage(&id, [("no-such-service", true)]).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        txn_code(exec(&session, &txn::check(&id)).await),
        code::INVALID
    );
    let short = exec(&session, &txn::apply(&id, Duration::from_secs(5))).await;
    assert_eq!(txn_code(short), code::BAD_REQUEST);
    assert_eq!(status(&session).await.open, None);
    assert_eq!(run(&session, &["cat", "/etc/config/system"]).await, system);
}

/// Last: it restarts the container, after which only `docker exec` reaches it.
#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn the_guard_rolls_back_an_open_transaction_at_boot() {
    let container = env::var("UNIWRT_NODE_CONTAINER").expect("UNIWRT_NODE_CONTAINER");
    let session = managed().await;
    let id = TxnId::new("boot").unwrap();
    let system = begin(&session, &id, Duration::from_secs(600)).await;
    wait_state(&session, OpenState::Applied).await;
    session.close().await;

    let docker = |args: &[&str]| Process::new("docker").args(args).output().unwrap();
    assert!(docker(&["restart", "-t", "2", &container]).status.success());
    let inside = |script: &str| docker(&["exec", &container, "sh", "-c", script]);
    let start = Instant::now();
    while !inside("ubus call network.interface.lan status | grep -q '\"up\": true'")
        .status
        .success()
    {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "the container did not boot"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
    let last = String::from_utf8(inside("cat /etc/uniwrt/journal/last").stdout).unwrap();
    assert!(
        last.contains("\"id\":\"boot\"") && last.contains("\"reason\":\"boot\""),
        "{last}"
    );
    assert!(
        !inside("[ -e /etc/uniwrt/journal/current ]")
            .status
            .success()
    );
    let restored = String::from_utf8(inside("cat /etc/config/system").stdout).unwrap();
    assert_eq!(restored, system);
    let start = Instant::now();
    while !inside(DNSMASQ_ON).status.success() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "dnsmasq does not run after boot"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}
