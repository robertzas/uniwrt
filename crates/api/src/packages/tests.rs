//! The package manager's endpoints on the demo: the locks, and a change started as a job on a
//! managed node, audited. The demo's dial doesn't reach the simulated nodes, so the catalog and
//! the job's run are the jobs crate's tests (and the sim's).

use std::time::Duration;

use axum::http::{StatusCode, header};
use serde_json::{Value, json};
use uniwrt_core::Timestamp;
use uniwrt_model::{
    inventory::{Inventory, Package},
    node::NodeMode,
};
use uniwrt_store::{AuditRepo as _, FactsRepo as _, InventoryRepo as _, NodeRepo as _};

use crate::testing::{Demo, json, post};

async fn packages(demo: &Demo, id: i64) -> Value {
    let res = demo
        .send(demo.get(&format!("/v1/nodes/{id}/packages")))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    json(res).await
}

fn find<'a>(listed: &'a Value, name: &str) -> Option<&'a Value> {
    listed["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == name)
}

#[tokio::test]
async fn packages_show_their_locks_and_change_by_a_job() {
    let demo = Demo::start().await;
    let ap = demo.ids[1];
    let id = ap.get();
    // The demo's adoption read no packages: read them, once the engine's facts are saved.
    let snapshots = uniwrt_engine::Snapshots::spawn(
        &demo.engine,
        demo.store.clone(),
        Duration::from_millis(20),
    );
    tokio::time::timeout(Duration::from_secs(30), async {
        while demo
            .store
            .latest_facts(ap)
            .await
            .unwrap()
            .is_none_or(|facts| facts.board.layout.lan.is_empty())
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the facts come");
    snapshots.stop().await;
    let package = |name: &str, explicit| Package {
        name: name.to_owned(),
        version: "1".to_owned(),
        explicit,
    };
    demo.store
        .set_inventory(
            ap,
            Inventory {
                taken_at: Timestamp::from_unix_ms(1_000),
                packages: vec![
                    package("base-files", false),
                    package("dropbear", false),
                    package("rpcd", true),
                ],
                feeds: Vec::new(),
            },
        )
        .await
        .unwrap();
    let listed = packages(&demo, id).await;
    assert_eq!(listed["required_unknown"], Value::Null, "{listed}");
    assert!(listed["taken_at"].is_number(), "{listed}");
    assert_eq!(find(&listed, "dropbear").unwrap()["lock"], "essential");
    assert_eq!(find(&listed, "rpcd").unwrap()["lock"], Value::Null);
    assert_eq!(find(&listed, "base-files").unwrap()["with_firmware"], true);

    let change = |body: Value| {
        post(
            &format!("/v1/nodes/{id}/packages"),
            &body,
            Some(&demo.cookie),
        )
    };
    let catalog = || {
        post(
            &format!("/v1/nodes/{id}/packages/catalog"),
            &json!({}),
            Some(&demo.cookie),
        )
    };
    // Observe mode: nothing is read or changed.
    let res = demo.send(catalog()).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let res = demo.send(change(json!({ "install": ["tcpdump"] }))).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    demo.store
        .set_node_mode(ap, NodeMode::Managed)
        .await
        .unwrap();
    for body in [
        json!({ "remove": ["dropbear"] }),
        json!({ "install": ["--force-depends"] }),
        json!({}),
    ] {
        let res = demo.send(change(body.clone())).await;
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    let res = demo.send(change(json!({ "purge": ["rpcd"] }))).await;
    assert!(res.status().is_client_error());

    let res = demo
        .send(change(
            json!({ "install": ["tcpdump"], "upgrade": ["dnsmasq"] }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let location = res.headers()[header::LOCATION].to_str().unwrap().to_owned();
    let job = json(res).await;
    assert_eq!(job["kind"], "packages");
    let audited = demo.store.audit_log(1).await.unwrap()[0]
        .detail
        .clone()
        .unwrap();
    assert!(
        audited.contains("install tcpdump; upgrade dnsmasq"),
        "{audited}"
    );

    assert_eq!(location, format!("/api/v1/jobs/{}", job["id"]));
}
