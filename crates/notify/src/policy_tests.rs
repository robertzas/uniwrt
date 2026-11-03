//! The policy over hand-made alerts on hand-moved time: who hears of an alert and when,
//! silences, acknowledgements, reminders, resolutions, reopening, rules that change, notifiers
//! added while alerts fire, and a restart.

use std::time::Duration;

use uniwrt_core::{AlertId, EventId, NodeId, NotifierId, RuleId, Timestamp, UserId};
use uniwrt_model::{
    alerts::{Acknowledgement, AlertCause, AlertRecord, AlertResolution, AlertState},
    events::EventLevel,
    notify::NotifierFilter,
};

use super::{Policy, Send};
use crate::Kind;

const N1: NotifierId = NotifierId::new(1);
const N2: NotifierId = NotifierId::new(2);

fn at(secs: u64) -> Timestamp {
    Timestamp::from_unix_ms(1_000_000_000).saturating_add(Duration::from_secs(secs))
}

fn alert(id: i64, level: EventLevel) -> AlertRecord {
    AlertRecord {
        id: AlertId::new(id),
        rule: RuleId::new(1),
        state: AlertState {
            name: "Device offline".to_owned(),
            level,
            subject: format!("node:{id}/offline"),
            node: Some(NodeId::new(id)),
            client: None,
            started_at: at(0),
            last_at: at(0),
            resolved_at: None,
            resolution: None,
            count: 1,
            silenced: false,
            cause: AlertCause::Event {
                event: EventId::new(1),
                kind: "node_offline".to_owned(),
                matches: 1,
                condition: None,
            },
        },
        acknowledged: None,
    }
}

fn resolved(mut alert: AlertRecord, why: AlertResolution, secs: u64) -> AlertRecord {
    alert.state.resolved_at = Some(at(secs));
    alert.state.resolution = Some(why);
    alert
}

/// N1 hears of warnings and up, reminded hourly; N2 only of critical ones, without reminders
/// or resolutions.
fn policy() -> Policy {
    let mut policy = Policy::new();
    policy.set_notifiers([
        (
            N1,
            NotifierFilter {
                remind_s: Some(3600),
                ..NotifierFilter::default()
            },
        ),
        (
            N2,
            NotifierFilter {
                min: EventLevel::Critical,
                resolved: false,
                remind_s: None,
                ..NotifierFilter::default()
            },
        ),
    ]);
    policy
}

fn sent(out: &[Send]) -> Vec<(NotifierId, Kind)> {
    out.iter().map(|s| (s.notifier, s.kind)).collect()
}

#[test]
fn opens_once_for_each_notifier_that_admits_it() {
    let mut policy = policy();
    let warning = alert(1, EventLevel::Warning);
    assert_eq!(
        sent(&policy.alerts(std::slice::from_ref(&warning), at(0))),
        [(N1, Kind::Opened)]
    );
    assert_eq!(policy.alerts(&[warning], at(5)), []);
    let critical = alert(2, EventLevel::Critical);
    assert_eq!(
        sent(&policy.alerts(&[critical], at(6))),
        [(N1, Kind::Opened), (N2, Kind::Opened)]
    );
    assert_eq!(policy.alerts(&[alert(3, EventLevel::Notice)], at(7)), []);
    let mut elsewhere = alert(4, EventLevel::Critical);
    elsewhere.state.node = None;
    let mut only_node_3 = Policy::new();
    only_node_3.set_notifiers([(
        N1,
        NotifierFilter {
            nodes: vec![NodeId::new(3)],
            ..NotifierFilter::default()
        },
    )]);
    assert_eq!(
        only_node_3.alerts(&[elsewhere, alert(1, EventLevel::Critical)], at(8)),
        []
    );
    assert_eq!(policy.firing_count(), 3);
}

#[test]
fn silenced_alerts_wait_until_maintenance_ends() {
    let mut policy = policy();
    let mut silenced = alert(1, EventLevel::Critical);
    silenced.state.silenced = true;
    assert_eq!(policy.alerts(std::slice::from_ref(&silenced), at(0)), []);
    assert_eq!(policy.tick(at(7200)), []);
    // Resolving while silenced tells nobody: nobody heard of it.
    let gone = resolved(silenced.clone(), AlertResolution::Cleared, 60);
    assert_eq!(policy.alerts(&[gone], at(60)), []);
    let mut again = alert(2, EventLevel::Critical);
    again.state.silenced = true;
    policy.alerts(std::slice::from_ref(&again), at(0));
    again.state.silenced = false;
    assert_eq!(
        sent(&policy.alerts(&[again], at(300))),
        [(N1, Kind::Opened), (N2, Kind::Opened)]
    );
}

#[test]
fn reminds_until_acknowledged() {
    let mut policy = policy();
    let firing = alert(1, EventLevel::Critical);
    policy.alerts(std::slice::from_ref(&firing), at(0));
    assert_eq!(policy.tick(at(3599)), []);
    let reminders = policy.tick(at(3600));
    assert_eq!(sent(&reminders), [(N1, Kind::Reminder)]);
    assert_eq!(policy.tick(at(3700)), []);
    assert_eq!(sent(&policy.tick(at(7200))), [(N1, Kind::Reminder)]);
    let mut acked = firing;
    acked.acknowledged = Some(Acknowledgement {
        by: UserId::new(1),
        username: "admin".to_owned(),
        at: at(7300),
    });
    assert_eq!(policy.alerts(std::slice::from_ref(&acked), at(7300)), []);
    assert_eq!(policy.tick(at(20_000)), []);
    // Its resolution still closes the loop for those who heard of it and want it.
    let gone = resolved(acked, AlertResolution::Cleared, 20_001);
    assert_eq!(
        sent(&policy.alerts(&[gone], at(20_001))),
        [(N1, Kind::Resolved)]
    );
    assert_eq!(policy.firing_count(), 0);
}

#[test]
fn reopening_tells_again() {
    let mut policy = policy();
    let firing = alert(1, EventLevel::Critical);
    policy.alerts(std::slice::from_ref(&firing), at(0));
    policy.alerts(
        &[resolved(firing.clone(), AlertResolution::Cleared, 30)],
        at(30),
    );
    let mut reopened = firing;
    reopened.state.count = 2;
    assert_eq!(
        sent(&policy.alerts(std::slice::from_ref(&reopened), at(60))),
        [(N1, Kind::Reopened), (N2, Kind::Reopened)]
    );
    // A reopen seen without its resolution (the bus lagged) tells again too.
    reopened.state.count = 3;
    assert_eq!(policy.alerts(&[reopened], at(90)).len(), 2);
}

#[test]
fn an_edited_rule_replaces_alerts_quietly() {
    let mut policy = policy();
    let firing = alert(1, EventLevel::Critical);
    policy.alerts(std::slice::from_ref(&firing), at(0));
    let changed = resolved(firing.clone(), AlertResolution::RuleChanged, 100);
    assert_eq!(policy.alerts(&[changed], at(100)), []);
    let replacement = AlertRecord {
        id: AlertId::new(9),
        ..firing.clone()
    };
    assert_eq!(
        policy.alerts(std::slice::from_ref(&replacement), at(110)),
        []
    );
    // It keeps the reminder schedule of the alert it replaced.
    assert_eq!(sent(&policy.tick(at(3600))), [(N1, Kind::Reminder)]);
    let gone = resolved(replacement, AlertResolution::Cleared, 4000);
    assert_eq!(
        sent(&policy.alerts(&[gone], at(4000))),
        [(N1, Kind::Resolved)]
    );
    // Too late to take over: a new alert.
    policy.alerts(
        &[resolved(firing.clone(), AlertResolution::RuleChanged, 5000)],
        at(5000),
    );
    let late = AlertRecord {
        id: AlertId::new(10),
        ..firing
    };
    policy.tick(at(5200));
    assert_eq!(policy.alerts(&[late], at(5200)).len(), 2);
}

#[test]
fn notifiers_added_hear_of_new_alerts_only() {
    let mut policy = Policy::new();
    policy.set_notifiers([(N1, NotifierFilter::default())]);
    let firing = alert(1, EventLevel::Critical);
    policy.alerts(std::slice::from_ref(&firing), at(0));
    policy.set_notifiers([
        (N1, NotifierFilter::default()),
        (N2, NotifierFilter::default()),
    ]);
    assert_eq!(policy.alerts(std::slice::from_ref(&firing), at(10)), []);
    assert_eq!(
        sent(&policy.alerts(&[alert(2, EventLevel::Critical)], at(20))),
        [(N1, Kind::Opened), (N2, Kind::Opened)]
    );
    // Removed notifiers are told nothing more.
    policy.set_notifiers([(N2, NotifierFilter::default())]);
    let gone = resolved(alert(2, EventLevel::Critical), AlertResolution::Cleared, 30);
    assert_eq!(
        sent(&policy.alerts(&[gone], at(30))),
        [(N2, Kind::Resolved)]
    );
    let gone = resolved(firing, AlertResolution::Cleared, 40);
    assert_eq!(policy.alerts(&[gone], at(40)), []);
}

#[test]
fn a_restart_does_not_repeat_itself() {
    let mut policy = policy();
    let firing = alert(1, EventLevel::Critical);
    let mut silenced = alert(2, EventLevel::Critical);
    silenced.state.silenced = true;
    policy.resume(vec![firing.clone(), silenced.clone()], at(1000));
    assert_eq!(policy.alerts(std::slice::from_ref(&firing), at(1001)), []);
    assert_eq!(sent(&policy.tick(at(4600))), [(N1, Kind::Reminder)]);
    silenced.state.silenced = false;
    assert_eq!(policy.alerts(&[silenced], at(4700)).len(), 2);
}
