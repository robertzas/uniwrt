//! Firmware upgrades through the router, on the simulated fleet against the fake ASU: the
//! catalog per node, ASU off, and the plan's gates before anything is read from a node.

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{Value, json};
use uniwrt_firmware::{Asu, testing::FakeAsu};

use crate::testing::{Demo, json as body, post};

async fn catalog(demo: &Demo) -> Value {
    let res = demo.send(demo.get("/v1/firmware")).await;
    assert_eq!(res.status(), StatusCode::OK);
    body(res).await
}

/// The catalog once every node's facts are in.
async fn settled(demo: &Demo) -> Value {
    for _ in 0..200 {
        let catalog = catalog(demo).await;
        let nodes = catalog["nodes"].as_array().unwrap();
        if !nodes.is_empty() && nodes.iter().all(|node| node["version"] != "") {
            return catalog;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the facts never came: {}", catalog(demo).await);
}

#[tokio::test]
async fn every_node_shows_its_release_and_the_upgrades_on_offer() {
    let fake = FakeAsu::start().await;
    let asu = Asu::new(fake.url(), "uniwrt/test").unwrap();
    let demo = Demo::start_full(false, Some(asu)).await;
    let catalog = settled(&demo).await;
    assert_eq!(catalog["error"], Value::Null);
    let nodes = catalog["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), demo.ids.len());
    let mut older = 0;
    for node in nodes {
        assert_eq!(node["distribution"], "OpenWrt");
        assert_eq!(node["target"], "mediatek/filogic");
        match node["version"].as_str().unwrap() {
            "24.10.8" => {
                older += 1;
                let offer = &node["offers"][0];
                assert_eq!(offer["release"], "25.12.6");
                assert_eq!(offer["kind"], "branch");
                let profile = node["board"].as_str().unwrap().replace(',', "_");
                assert_eq!(offer["profile"], profile.as_str());
                assert_eq!(offer["image_bytes"], 10_598_720);
                assert!(offer["notes"].as_str().unwrap().ends_with("notes-25.12.6"));
                assert_eq!(node["note"], Value::Null);
            }
            "25.12.6" => assert_eq!(node["offers"], json!([])),
            other => panic!("the sim runs {other}"),
        }
    }
    assert!(older > 0, "some node runs 24.10: {catalog}");
}

#[tokio::test]
async fn without_asu_the_catalog_says_so() {
    let demo = Demo::start_full(false, None).await;
    let catalog = settled(&demo).await;
    assert!(catalog["error"].as_str().unwrap().contains("--asu-url off"));
    assert!(
        catalog["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|node| node["offers"] == json!([]))
    );
}

#[tokio::test]
async fn a_plan_is_refused_before_reading_the_node_unless_the_release_is_on_offer() {
    let fake = FakeAsu::start().await;
    let asu = Asu::new(fake.url(), "uniwrt/test").unwrap();
    let demo = Demo::start_full(false, Some(asu)).await;
    let catalog = settled(&demo).await;
    let nodes = catalog["nodes"].as_array().unwrap();
    let upgradable = nodes
        .iter()
        .find(|node| node["offers"][0]["profile"] != Value::Null)
        .expect("a node with an image on offer");
    let id = upgradable["node"].as_i64().unwrap();
    let plan = |release: &str| {
        post(
            &format!("/v1/firmware/nodes/{id}/plan"),
            &json!({ "release": release }),
            Some(&demo.cookie),
        )
    };

    for release in ["25.12.5", "26.06.0", "../x"] {
        let res = demo.send(plan(release)).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{release}");
    }
    // On offer: the next step reads the node, which this controller can't reach.
    let res = demo.send(plan("25.12.6")).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert!(
        body(res).await["detail"]
            .as_str()
            .unwrap()
            .contains("cannot reach")
    );

    let res = demo
        .send(post(
            "/v1/firmware/nodes/999/plan",
            &json!({"release": "25.12.6"}),
            Some(&demo.cookie),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = demo
        .send(post(
            &format!("/v1/firmware/nodes/{id}/plan"),
            &json!({"release": "25.12.6"}),
            None,
        ))
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(fake.builds(), [] as [serde_json::Value; 0]);
}
