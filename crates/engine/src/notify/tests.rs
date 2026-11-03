//! Notifications over a store, a vault and an engine without nodes, fed alerts by hand on a
//! manual clock, delivering to a webhook on loopback: an alert already firing at the start is
//! not repeated, a failed delivery is tried again, alerts that arrive together go as a digest,
//! reminders follow the clock, a disabled notifier hears nothing, and a test goes out at once.

use std::{collections::VecDeque, net::SocketAddr, sync::Arc, time::Duration};

use serde_json::Value;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    sync::{Mutex, mpsc},
};
use uniwrt_core::{
    AlertId, Clock as _, EventId, ManualClock, NodeId, NotifierId, RuleId, Timestamp,
};
use uniwrt_model::{
    alerts::{AlertCause, AlertRecord, AlertResolution, AlertState, AlertWrite},
    events::EventLevel,
    notify::{NotifierConfig, NotifierFilter, NotifierSpec, WebhookFormat, WebhookSpec},
};
use uniwrt_notify::{Secrets, Sender};
use uniwrt_store::{AlertRepo as _, MasterKey, NotifierRepo as _, Store, Vault, VaultRepo as _};

use super::{Notifications, NotifyConfig};
use crate::{Engine, EngineConfig, Event};

const START: Timestamp = Timestamp::from_unix_secs(1_800_000_000);

/// A webhook on loopback: each request's JSON body, answered with the next of `statuses`
/// (then 204).
async fn webhook(statuses: &[&'static str]) -> (SocketAddr, mpsc::UnboundedReceiver<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let statuses = Arc::new(Mutex::new(
        statuses.iter().copied().collect::<VecDeque<_>>(),
    ));
    let (seen, bodies) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let (mut tcp, _) = listener.accept().await.unwrap();
            let statuses = Arc::clone(&statuses);
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut data = Vec::new();
                let mut chunk = [0; 8192];
                let (end, length) = loop {
                    let n = tcp.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    data.extend_from_slice(&chunk[..n]);
                    if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&data[..end]).to_lowercase();
                        let length = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length: ")?.trim().parse().ok())
                            .unwrap_or(0);
                        break (end, length);
                    }
                };
                while data.len() < end + 4 + length {
                    let n = tcp.read(&mut chunk).await.unwrap();
                    data.extend_from_slice(&chunk[..n]);
                }
                let status = statuses
                    .lock()
                    .await
                    .pop_front()
                    .unwrap_or("204 No Content");
                let body = serde_json::from_slice(&data[end + 4..]).unwrap();
                let response =
                    format!("HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
                tcp.write_all(response.as_bytes()).await.unwrap();
                let _ = seen.send(body);
            });
        }
    });
    (addr, bodies)
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
            started_at: START,
            last_at: START,
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

fn config(format: WebhookFormat) -> NotifierConfig {
    NotifierConfig {
        name: "Hook".to_owned(),
        enabled: true,
        spec: NotifierSpec::Webhook(WebhookSpec {
            format,
            template: None,
        }),
        filter: NotifierFilter {
            remind_s: Some(900),
            ..NotifierFilter::default()
        },
    }
}

async fn next(bodies: &mut mpsc::UnboundedReceiver<Value>) -> Value {
    tokio::time::timeout(Duration::from_secs(10), bodies.recv())
        .await
        .expect("no request came")
        .unwrap()
}

async fn delivered(store: &Store, id: NotifierId) {
    for _ in 0..500 {
        let status = store.notifier(id).await.unwrap().unwrap().status;
        if status.last_ok_at.is_some() {
            assert_eq!(status.last_error, None);
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the delivery was not recorded");
}

#[tokio::test(flavor = "multi_thread")]
async fn notifies_through_a_webhook() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(START));
    let store = Store::open(dir.path().join("test.db"), clock.clone())
        .await
        .unwrap();
    let vault = Vault::open(&store, &MasterKey::generate().unwrap())
        .await
        .unwrap();
    let (addr, mut bodies) = webhook(&["503 Service Unavailable"]).await;
    let notifier = store
        .create_notifier(config(WebhookFormat::Generic))
        .await
        .unwrap();
    let mut secrets = Secrets::default();
    secrets.url = Some(format!("http://{addr}/hook"));
    vault
        .put_secret(&Secrets::vault_name(notifier.id), secrets.to_json())
        .await
        .unwrap();
    // Already firing (and so already notified) before the start.
    let before = alert(1, EventLevel::Critical);
    store
        .record_alerts(vec![AlertWrite::Open {
            rule: before.rule,
            state: before.state.clone(),
        }])
        .await
        .unwrap();

    let engine = Engine::<crate::testing::SharedLink>::new(EngineConfig::default(), clock.clone());
    let settings = NotifyConfig {
        tick_every: Duration::from_millis(5),
        gather: Duration::from_millis(50),
        retries: vec![Duration::from_millis(10)],
        queue: 16,
    };
    let sender = Sender::new("uniwrt/test").unwrap();
    let notifications =
        Notifications::start(&engine, store.clone(), vault.clone(), sender, settings).await;
    let publish = |alerts: Vec<AlertRecord>| {
        engine.context().publish(Event::Alerts {
            alerts: alerts.into(),
        });
    };

    publish(vec![before.clone(), alert(2, EventLevel::Critical)]);
    // The first try fails with 503; the second goes through.
    let first = next(&mut bodies).await;
    let second = next(&mut bodies).await;
    assert_eq!(first, second);
    assert_eq!(second["event"], "opened");
    assert_eq!(second["title"], "Device offline · Node 2");
    assert_eq!(second["alerts"][0]["id"], 2);
    delivered(&store, notifier.id).await;

    // Together: a digest.
    let mut gone = alert(2, EventLevel::Critical);
    gone.state.resolved_at = Some(START);
    gone.state.resolution = Some(AlertResolution::Cleared);
    publish(vec![alert(3, EventLevel::Warning), gone]);
    let digest = next(&mut bodies).await;
    assert_eq!(digest["event"], "digest");
    assert_eq!(digest["title"], "2 alerts: 1 firing, 1 resolved");

    // Reminders of what still fires: alert 1 (heard of before the start) and alert 3.
    clock.advance(Duration::from_mins(15));
    let reminder = next(&mut bodies).await;
    assert_eq!(reminder["event"], "digest", "{reminder}");
    let ids: Vec<_> = reminder["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["id"].as_i64().unwrap(),
                a["event"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        ids,
        [(1, "reminder".to_owned()), (3, "reminder".to_owned())]
    );

    // Disabled: nothing more.
    let handle = notifications.handle();
    let disabled = NotifierConfig {
        enabled: false,
        ..config(WebhookFormat::Generic)
    };
    store.update_notifier(notifier.id, disabled).await.unwrap();
    handle.reload().await.unwrap();
    publish(vec![alert(4, EventLevel::Critical)]);
    let quiet = tokio::time::timeout(Duration::from_millis(300), bodies.recv()).await;
    assert!(quiet.is_err(), "a disabled notifier was sent {quiet:?}");

    let spec = config(WebhookFormat::Generic).spec;
    handle
        .test(&spec, &secrets, Some("https://uniwrt.example"))
        .await
        .unwrap();
    let test = next(&mut bodies).await;
    assert_eq!(test["event"], "test");
    assert_eq!(test["url"], "https://uniwrt.example/insights/alerts");
    assert!(clock.now() > START);
    notifications.stop().await;
}
