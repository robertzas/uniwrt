//! Firmware upgrades through the router, on the simulated fleet against the fake ASU: the
//! catalog per node, ASU off, and the plan's gates before anything is read from a node.

use std::time::Duration;

use axum::{body::Body, http::Method, http::StatusCode};
use serde_json::{Value, json};
use uniwrt_firmware::{
    Asu,
    testing::{FakeAsu, sysupgrade_image},
};

use crate::testing::{Demo, json as body, post, request};

async fn catalog(demo: &Demo) -> Value {
    let res = demo.send(demo.get("/v1/firmware")).await;
    assert_eq!(res.status(), StatusCode::OK);
    body(res).await
}

/// The catalog once every node's facts are in.
pub(crate) async fn settled(demo: &Demo) -> Value {
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
            &json!({ "source": "asu", "release": release }),
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
            &json!({"source": "official", "release": "25.12.6"}),
            Some(&demo.cookie),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = demo
        .send(post(
            &format!("/v1/firmware/nodes/{id}/plan"),
            &json!({"source": "asu", "release": "25.12.6"}),
            None,
        ))
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(fake.builds(), [] as [serde_json::Value; 0]);
}

fn upload(demo: &Demo, name: &str, image: Vec<u8>) -> axum::extract::Request {
    let uri = format!("/v1/firmware/uploads?name={name}");
    let (parts, _) = request(Method::POST, &uri, None, Some(&demo.cookie)).into_parts();
    axum::extract::Request::from_parts(parts, Body::from(image))
}

#[tokio::test]
async fn an_uploaded_image_is_offered_to_the_nodes_it_fits_and_planned_without_reading_them() {
    // Without ASU: uploads don't need it.
    let demo = Demo::start_full(false, None).await;
    let settled = settled(&demo).await;
    let nodes = settled["nodes"].as_array().unwrap();
    let board = nodes[0]["board"].as_str().unwrap().to_owned();
    let other = nodes
        .iter()
        .find(|node| node["board"] != board.as_str())
        .map(|node| node["node"].as_i64().unwrap());

    let res = demo
        .send(upload(&demo, "notes.txt", b"not an image".to_vec()))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert!(
        body(res).await["detail"]
            .as_str()
            .unwrap()
            .contains("metadata")
    );

    let image = sysupgrade_image(&board, "25.12.7", 50_000);
    let res = demo.send(upload(&demo, "custom.bin", image.clone())).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let kept = body(res).await;
    assert_eq!(kept["name"], "custom.bin");
    assert_eq!(kept["boards"], json!([board]));
    assert_eq!(kept["version"], "25.12.7");
    let sha256 = kept["sha256"].as_str().unwrap().to_owned();

    let listed = catalog(&demo).await;
    assert_eq!(listed["uploads"], json!([kept]));
    for node in listed["nodes"].as_array().unwrap() {
        let fits = node["board"] == board.as_str();
        assert_eq!(node["uploads"] == json!([sha256]), fits, "{node}");
    }

    let plan = |id: i64| {
        post(
            &format!("/v1/firmware/nodes/{id}/plan"),
            &json!({"source": "upload", "sha256": sha256}),
            Some(&demo.cookie),
        )
    };
    let id = nodes[0]["node"].as_i64().unwrap();
    let res = demo.send(plan(id)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let planned = body(res).await;
    assert_eq!(planned["source"], "upload");
    assert_eq!(planned["release"], "25.12.7");
    assert_eq!(planned["image_bytes"], image.len());
    assert_eq!(planned["packages_at"], Value::Null);
    assert_eq!(planned["upload"]["sha256"], sha256.as_str());
    if let Some(other) = other {
        assert_eq!(demo.send(plan(other)).await.status(), StatusCode::CONFLICT);
    }

    let gone = format!("/v1/firmware/uploads/{sha256}");
    let res = demo
        .send(request(Method::DELETE, &gone, None, Some(&demo.cookie)))
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = demo
        .send(request(Method::DELETE, &gone, None, Some(&demo.cookie)))
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(demo.send(plan(id)).await.status(), StatusCode::NOT_FOUND);
    assert_eq!(catalog(&demo).await["uploads"], json!([]));
}
