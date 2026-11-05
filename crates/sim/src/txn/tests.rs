//! The simulated tool answers the node crate's commands as the real one does.

use std::time::Duration;

use uniwrt_node::{
    Error,
    txn::{self, Footprint, Outcome, Reason, TxnId, code, current},
};

use super::{Confirmed, Now, Tool};
use crate::hand_edit::{AFTER_S, HandEdit};

const T0: Now = Now {
    uptime_s: 100,
    unix_s: 1_800_000_000,
    booted_ms: 1,
};

fn at(uptime_s: u64) -> Now {
    Now { uptime_s, ..T0 }
}

fn run<T: serde::de::DeserializeOwned>(
    tool: &mut Tool,
    command: &txn::Command<T>,
    now: Now,
) -> uniwrt_node::Result<T> {
    let request = &command.request;
    let output = tool
        .exec(&request.argv, request.stdin.as_deref(), now)
        .expect("a command of the tool");
    command.parse(output)
}

fn managed() -> Tool {
    let mut tool = Tool::stock();
    tool.files.insert("network".into(), "old network\n".into());
    run(&mut tool, &Footprint::get().install(), T0).unwrap();
    tool
}

fn applied(tool: &mut Tool, id: &TxnId) {
    run(tool, &txn::stage(id, [("dnsmasq", false)]).unwrap(), T0).unwrap();
    run(tool, &txn::put(id, "network", "new network\n").unwrap(), T0).unwrap();
    run(
        tool,
        &txn::put(id, "wireless", "new wireless\n").unwrap(),
        T0,
    )
    .unwrap();
    let checked = run(tool, &txn::check(id), T0).unwrap();
    assert_eq!(checked.packages, ["network", "wireless"]);
    let armed = run(tool, &txn::apply(id, Duration::from_secs(120)), T0).unwrap();
    assert_eq!(armed.timeout_s, 120);
}

fn code_of<T: std::fmt::Debug>(result: uniwrt_node::Result<T>) -> String {
    match result {
        Err(Error::Txn { code, .. }) => code,
        other => panic!("expected the tool's error, got {other:?}"),
    }
}

#[test]
fn a_confirmed_transaction_keeps_its_files_and_services() {
    let mut tool = managed();
    let id = TxnId::new("j1").unwrap();
    applied(&mut tool, &id);
    assert_eq!(tool.files["network"], "new network\n");
    assert!(!tool.services["dnsmasq"]);
    let status = run(&mut tool, &txn::status(), at(110)).unwrap().unwrap();
    let open = status.open.unwrap();
    assert_eq!((open.id.as_str(), open.remaining_s), ("j1", 110));
    run(&mut tool, &txn::confirm(&id), at(110)).unwrap();
    let status = run(&mut tool, &txn::status(), at(500)).unwrap().unwrap();
    assert_eq!(status.open, None);
    assert_eq!(status.last.unwrap().outcome, Outcome::Confirmed);
    assert_eq!(tool.files["wireless"], "new wireless\n");
    let confirmed = Confirmed {
        unix_s: T0.unix_s,
        packages: vec!["network".into(), "wireless".into()],
    };
    assert_eq!(tool.confirmed(), Some(&confirmed));
}

#[test]
fn the_hand_edit_comes_a_minute_after_the_first_confirmed_apply_once() {
    let mut tool = managed();
    let mut edit = HandEdit::default();
    assert_eq!(
        edit.step(&mut tool, T0.unix_s + 600),
        None,
        "nothing confirmed yet"
    );
    let id = TxnId::new("j1").unwrap();
    let system = "\nconfig system\n\toption hostname 'ap'\n\toption log_size '128'\n";
    run(&mut tool, &txn::stage(&id, []).unwrap(), T0).unwrap();
    run(&mut tool, &txn::put(&id, "system", system).unwrap(), T0).unwrap();
    run(&mut tool, &txn::check(&id), T0).unwrap();
    run(&mut tool, &txn::apply(&id, Duration::from_secs(120)), T0).unwrap();
    assert_eq!(edit.step(&mut tool, T0.unix_s + 600), None, "still open");
    run(&mut tool, &txn::confirm(&id), T0).unwrap();
    assert_eq!(edit.step(&mut tool, T0.unix_s + AFTER_S - 1), None);
    assert_eq!(
        edit.step(&mut tool, T0.unix_s + AFTER_S).as_deref(),
        Some("system")
    );
    assert!(tool.files["system"].contains("option hostname 'ap-1'"));
    let edited = tool.files["system"].clone();
    assert_eq!(edit.step(&mut tool, T0.unix_s + 3600), None, "once");
    assert_eq!(tool.files["system"], edited);
}

#[test]
fn a_rollback_restores_files_absent_files_and_services() {
    for (reason, now) in [
        (Reason::Requested, at(110)),
        (Reason::Timeout, at(220)),
        (
            Reason::Boot,
            Now {
                booted_ms: 2,
                ..at(5)
            },
        ),
    ] {
        let mut tool = managed();
        let id = TxnId::new("j1").unwrap();
        applied(&mut tool, &id);
        if reason == Reason::Requested {
            run(&mut tool, &txn::rollback(&id), now).unwrap();
        }
        let last = run(&mut tool, &txn::status(), now)
            .unwrap()
            .unwrap()
            .last
            .unwrap();
        assert_eq!(
            (last.outcome, last.reason),
            (Outcome::RolledBack, Some(reason))
        );
        assert_eq!(tool.files["network"], "old network\n");
        assert!(!tool.files.contains_key("wireless"));
        assert!(tool.services["dnsmasq"]);
        assert_eq!(
            code_of(run(&mut tool, &txn::confirm(&id), now)),
            code::EXPIRED
        );
    }
}

#[test]
fn extend_moves_the_deadline_and_refusals_carry_the_tools_codes() {
    let mut tool = managed();
    let id = TxnId::new("j1").unwrap();
    let other = TxnId::new("j2").unwrap();
    assert_eq!(
        code_of(run(&mut tool, &txn::check(&id), T0)),
        code::NOT_FOUND
    );
    run(
        &mut tool,
        &txn::stage(&id, [("nothere", true)]).unwrap(),
        T0,
    )
    .unwrap();
    assert_eq!(code_of(run(&mut tool, &txn::check(&id), T0)), code::INVALID);
    assert_eq!(
        code_of(run(
            &mut tool,
            &txn::apply(&id, Duration::from_secs(120)),
            T0
        )),
        code::BAD_REQUEST
    );
    applied(&mut tool, &id);
    assert_eq!(
        code_of(run(&mut tool, &txn::stage(&other, []).unwrap(), T0)),
        code::BUSY
    );
    assert_eq!(code_of(run(&mut tool, &txn::uninstall(), T0)), code::BUSY);
    run(
        &mut tool,
        &txn::extend(&id, Duration::from_secs(300)),
        at(200),
    )
    .unwrap();
    assert_eq!(tool.open(), Some("j1"));
    run(&mut tool, &txn::status(), at(499)).unwrap();
    assert_eq!(tool.open(), Some("j1"));
    run(&mut tool, &txn::status(), at(500)).unwrap();
    assert_eq!(tool.open(), None);
}

#[test]
fn reads_report_files_and_services() {
    let mut tool = managed();
    let read = current::packages(&["network", "dhcp"]).unwrap();
    let output = tool.exec(&read.request.argv, None, T0).unwrap();
    let files = read.parse(output).unwrap();
    assert_eq!(files["network"].as_deref(), Some("old network\n"));
    assert_eq!(files["dhcp"], None);
    let read = current::services(&["firewall", "usteer"]).unwrap();
    let output = tool.exec(&read.request.argv, None, T0).unwrap();
    let services = read.parse(output).unwrap();
    assert_eq!(services["firewall"], current::ServiceState::On);
    assert_eq!(services["usteer"], current::ServiceState::Absent);
    let unmanaged = Tool::stock();
    assert!(
        run(&mut unmanaged.clone(), &txn::status(), T0)
            .unwrap()
            .is_none()
    );
    assert!(
        unmanaged
            .clone()
            .exec(
                &txn::check(&TxnId::new("j1").unwrap()).request.argv,
                None,
                T0
            )
            .is_none()
    );
}
