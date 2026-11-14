//! The floor plan routes over the demo: floors added, edited and deleted, a plan uploaded and
//! served, nodes placed and taken off, and what is refused.

use axum::{
    body::{self, Body},
    http::{Method, StatusCode, header},
};
use serde_json::{Value, json};
use uniwrt_model::user::AuditAction;
use uniwrt_store::AuditRepo as _;

use super::image::tests::{png, webp};
use crate::testing::{Demo, json, request};

async fn send(demo: &Demo, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let res = demo
        .send(request(method, uri, body.as_ref(), Some(&demo.cookie)))
        .await;
    let status = res.status();
    let bytes = body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn upload(demo: &Demo, id: &Value, data: Vec<u8>) -> axum::extract::Request {
    let (parts, _) = request(
        Method::PUT,
        &format!("/v1/floors/{id}/image"),
        None,
        Some(&demo.cookie),
    )
    .into_parts();
    axum::extract::Request::from_parts(parts, Body::from(data))
}

fn fields(name: &str, level: i16) -> Value {
    json!({ "name": name, "level": level, "height_m": 3.0, "scale": null })
}

#[tokio::test]
async fn floors_are_added_edited_and_deleted() {
    let demo = Demo::start().await;
    let (status, ground) = send(
        &demo,
        Method::POST,
        "/v1/floors",
        Some(fields(" Ground ", 0)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        (ground["name"].as_str(), ground["image"].is_null()),
        (Some("Ground"), true)
    );
    let id = &ground["id"];

    let (status, _) = send(&demo, Method::POST, "/v1/floors", Some(fields("Again", 0))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = send(&demo, Method::POST, "/v1/floors", Some(fields("", 1))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // No plan yet, so no scale.
    let mut scaled = fields("Ground", 0);
    scaled["scale"] = json!({ "from": { "x": 0, "y": 0 }, "to": { "x": 400, "y": 0 }, "length_m": 10, "unit": "ft" });
    let uri = format!("/v1/floors/{id}");
    let (status, _) = send(&demo, Method::PUT, &uri, Some(scaled.clone())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let res = demo.send(upload(&demo, id, webp(800, 600))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let (status, floor) = send(&demo, Method::PUT, &uri, Some(scaled)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(floor["scale"]["unit"], "ft");
    assert_eq!(floor["image"]["version"], 1);

    let (status, list) = send(&demo, Method::GET, "/v1/floors", None).await;
    assert_eq!(
        (status, list.as_array().unwrap().len()),
        (StatusCode::OK, 1)
    );
    let (status, _) = send(&demo, Method::DELETE, &uri, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(&demo, Method::DELETE, &uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let audit = demo.store.audit_log(20).await.unwrap();
    let details: Vec<_> = audit
        .iter()
        .filter(|entry| entry.action == AuditAction::FloorPlan)
        .filter_map(|entry| entry.detail.as_deref())
        .collect();
    assert_eq!(
        details,
        [
            "deleted floor Ground (level 0)",
            "changed floor Ground (level 0)",
            "uploaded the plan of floor Ground (800 × 600 px)",
            "added floor Ground (level 0)",
        ]
    );
}

#[tokio::test]
async fn a_plan_is_checked_served_and_carries_its_nodes() {
    let demo = Demo::start().await;
    let (_, floor) = send(&demo, Method::POST, "/v1/floors", Some(fields("Ground", 0))).await;
    let id = &floor["id"];
    let node = demo.ids[1];
    let place = format!("/v1/floors/{id}/nodes/{node}");
    let at = json!({ "x": 400, "y": 300 });

    let (status, _) = send(&demo, Method::PUT, &place, Some(at.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT);
    for refused in [b"<svg/>".to_vec(), png(8, 600), webp(4097, 10)] {
        let res = demo.send(upload(&demo, id, refused)).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
    let res = demo.send(upload(&demo, id, png(800, 600))).await;
    assert_eq!(res.status(), StatusCode::OK);

    let (status, floor) = send(&demo, Method::PUT, &place, Some(at)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        floor["nodes"],
        json!([{ "node": node, "at": { "x": 400.0, "y": 300.0 } }])
    );
    let (status, _) = send(
        &demo,
        Method::PUT,
        &place,
        Some(json!({ "x": 801, "y": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(
        &demo,
        Method::PUT,
        &format!("/v1/floors/{id}/nodes/999"),
        Some(json!({ "x": 1, "y": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The same drawing at twice the size: the node follows.
    let res = demo.send(upload(&demo, id, webp(1600, 1200))).await;
    let floor = json(res).await;
    assert_eq!(floor["nodes"][0]["at"], json!({ "x": 800.0, "y": 600.0 }));
    assert_eq!(floor["image"]["version"], 2);

    let res = demo
        .send(demo.get(&format!("/v1/floors/{id}/image?v=2")))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "image/webp");
    assert!(
        res.headers()[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .contains("immutable")
    );
    let bytes = body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    assert_eq!(bytes.as_ref(), webp(1600, 1200).as_slice());

    let (status, _) = send(&demo, Method::DELETE, &place, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(&demo, Method::DELETE, &place, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn coverage_is_fitted_to_what_placed_nodes_hear() {
    let demo = Demo::start().await;
    let (status, coverage) = send(&demo, Method::GET, "/v1/floors/coverage", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        coverage["bands"]
            .as_array()
            .unwrap()
            .iter()
            .all(|b| b["samples"] == 0)
    );

    let (_, floor) = send(&demo, Method::POST, "/v1/floors", Some(fields("Ground", 0))).await;
    let id = floor["id"].clone();
    demo.send(upload(&demo, &id, png(800, 600))).await;
    let mut scaled = fields("Ground", 0);
    scaled["scale"] = json!({
        "from": { "x": 0, "y": 0 }, "to": { "x": 400, "y": 0 }, "length_m": 20, "unit": "m"
    });
    let (status, _) = send(
        &demo,
        Method::PUT,
        &format!("/v1/floors/{id}"),
        Some(scaled),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for (i, x) in [(0, 100), (1, 500), (2, 700)] {
        let place = format!("/v1/floors/{id}/nodes/{}", demo.ids[i]);
        let (status, _) = send(
            &demo,
            Method::PUT,
            &place,
            Some(json!({ "x": x, "y": 300 })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    // The demo's scans hear the nodes within seconds.
    let fitted = async {
        loop {
            let (_, coverage) = send(&demo, Method::GET, "/v1/floors/coverage", None).await;
            let bands = coverage["bands"].as_array().cloned().unwrap_or_default();
            if let Some(band) = bands.into_iter().find(|b| b["samples"].as_u64() > Some(0)) {
                return band;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    };
    let band = tokio::time::timeout(std::time::Duration::from_secs(30), fitted)
        .await
        .expect("a band fitted within 30 s");
    assert_eq!(band["exponent_fitted"], true);
    assert!(band["error_db"].is_number());
    assert_ne!(band["transmitters"], json!([]));
}

#[tokio::test]
async fn walls_are_checked_saved_and_follow_the_plan() {
    let demo = Demo::start().await;
    let (_, floor) = send(&demo, Method::POST, "/v1/floors", Some(fields("Ground", 0))).await;
    let id = &floor["id"];
    let uri = format!("/v1/floors/{id}/walls");
    let wall =
        json!({ "from": { "x": 0, "y": 300 }, "to": { "x": 800, "y": 300 }, "material": "brick" });

    let (status, _) = send(&demo, Method::PUT, &uri, Some(json!([wall]))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    demo.send(upload(&demo, id, png(800, 600))).await;
    let (status, floor) = send(&demo, Method::PUT, &uri, Some(json!([wall]))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(floor["walls"][0]["material"], "brick");
    for refused in [
        json!([{ "from": { "x": 0, "y": 0 }, "to": { "x": 900, "y": 0 }, "material": "wood" }]),
        json!([{ "from": { "x": 0, "y": 0 }, "to": { "x": 9, "y": 0 }, "material": "paper" }]),
        Value::Array(vec![wall.clone(); 1001]),
    ] {
        let (status, _) = send(&demo, Method::PUT, &uri, Some(refused)).await;
        assert!(
            status.is_client_error() && status != StatusCode::CONFLICT,
            "{status}"
        );
    }
    let (status, _) = send(&demo, Method::PUT, "/v1/floors/999/walls", Some(json!([]))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The same drawing at twice the size: the wall follows.
    let res = demo.send(upload(&demo, id, webp(1600, 1200))).await;
    assert_eq!(
        json(res).await["walls"][0]["to"],
        json!({ "x": 1600.0, "y": 600.0 })
    );
    let (status, floor) = send(&demo, Method::PUT, &uri, Some(json!([]))).await;
    assert_eq!(
        (status, floor["walls"].clone()),
        (StatusCode::OK, json!([]))
    );
}
