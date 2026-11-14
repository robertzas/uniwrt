//! Floors over a real database: created, listed by level, edited, given an image that carries
//! what was drawn on it, nodes placed and moved, and gone with their floor or node.

use uniwrt_core::{FloorId, NodeId};
use uniwrt_model::floor::{
    FloorFields, FloorScale, ImageKind, LengthUnit, Material, Placement, Point, Wall,
};

use crate::{
    Error, FloorRepo as _, NewFloorImage, NodeRepo as _, repo::nodes::tests::new_node,
    testing::open,
};

fn fields(name: &str, level: i16) -> FloorFields {
    FloorFields {
        name: name.to_owned(),
        level,
        height_m: 3.0,
        scale: None,
    }
}

fn image(width_px: u32, height_px: u32, data: &[u8]) -> NewFloorImage {
    NewFloorImage {
        kind: ImageKind::Webp,
        width_px,
        height_px,
        data: data.to_vec(),
    }
}

fn placed(node: NodeId, x: f64, y: f64) -> Placement {
    Placement {
        node,
        at: Point { x, y },
    }
}

#[tokio::test]
async fn floors_are_kept_by_level_and_levels_are_unique() {
    let (_dir, store, _) = open().await;
    let upstairs = store.create_floor(fields("Upstairs", 1), 3).await.unwrap();
    let ground = store.create_floor(fields("Ground", 0), 3).await.unwrap();
    assert_eq!(ground.image, None);
    let names: Vec<_> = store
        .floors()
        .await
        .unwrap()
        .into_iter()
        .map(|floor| floor.name)
        .collect();
    assert_eq!(names, ["Ground", "Upstairs"]);

    let taken = store.create_floor(fields("Attic", 1), 3).await.unwrap_err();
    assert!(matches!(
        taken,
        Error::Conflict {
            what: "a floor at level",
            ..
        }
    ));
    let taken = store
        .update_floor(ground.id, fields("Ground", 1))
        .await
        .unwrap_err();
    assert!(matches!(taken, Error::Conflict { .. }));
    let renamed = store
        .update_floor(upstairs.id, fields("First", 2))
        .await
        .unwrap()
        .unwrap();
    assert_eq!((renamed.name.as_str(), renamed.level), ("First", 2));
    assert_eq!(
        store
            .update_floor(FloorId::new(99), fields("x", 5))
            .await
            .unwrap(),
        None
    );

    store.create_floor(fields("Basement", -1), 3).await.unwrap();
    let full = store.create_floor(fields("Roof", 3), 3).await.unwrap_err();
    assert!(matches!(
        full,
        Error::Conflict {
            what: "the most floors",
            ..
        }
    ));

    assert!(store.delete_floor(upstairs.id).await.unwrap());
    assert!(!store.delete_floor(upstairs.id).await.unwrap());
    assert_eq!(store.floors().await.unwrap().len(), 2);
}

#[tokio::test]
async fn a_new_image_carries_the_scale_and_the_placements() {
    let (_dir, store, _) = open().await;
    let a = store.create_node(new_node(1)).await.unwrap().id;
    let floor = store.create_floor(fields("Ground", 0), 32).await.unwrap();
    assert_eq!(store.floor_image(floor.id).await.unwrap(), None);

    let first = store
        .set_floor_image(floor.id, image(800, 600, b"one"))
        .await
        .unwrap()
        .unwrap();
    let stored = first.image.unwrap();
    assert_eq!((stored.width_px, stored.bytes, stored.version), (800, 3, 1));
    let scale = FloorScale {
        from: Point { x: 100.0, y: 100.0 },
        to: Point { x: 500.0, y: 100.0 },
        length_m: 10.0,
        unit: LengthUnit::M,
    };
    store
        .update_floor(
            floor.id,
            FloorFields {
                scale: Some(scale),
                ..fields("Ground", 0)
            },
        )
        .await
        .unwrap();
    store
        .place_node(floor.id, placed(a, 400.0, 300.0))
        .await
        .unwrap();
    let wall = Wall {
        from: Point { x: 0.0, y: 300.0 },
        to: Point { x: 800.0, y: 300.0 },
        material: Material::Brick,
    };
    let walled = store
        .set_floor_walls(floor.id, vec![wall])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(walled.walls, [wall]);
    assert_eq!(store.floors().await.unwrap()[0].walls, [wall]);
    assert_eq!(
        store
            .set_floor_walls(FloorId::new(99), Vec::new())
            .await
            .unwrap(),
        None
    );

    let second = store
        .set_floor_image(floor.id, image(1600, 900, b"second"))
        .await
        .unwrap()
        .unwrap();
    let (meta, data) = store.floor_image(floor.id).await.unwrap().unwrap();
    assert_eq!(
        (meta.version, meta.height_px, data.as_slice()),
        (2, 900, &b"second"[..])
    );
    assert_eq!(second.nodes, [placed(a, 800.0, 450.0)]);
    assert_eq!(
        second.walls,
        [Wall {
            from: Point { x: 0.0, y: 450.0 },
            to: Point {
                x: 1600.0,
                y: 450.0
            },
            ..wall
        }]
    );
    let scale = second.scale.unwrap();
    assert_eq!(scale.from, Point { x: 200.0, y: 150.0 });
    assert!((scale.meters_per_px() - 10.0 / 800.0).abs() < 1e-12);
    assert_eq!(
        store
            .set_floor_image(FloorId::new(99), image(16, 16, b"x"))
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn a_node_is_on_one_floor_and_leaves_with_its_floor_or_itself() {
    let (_dir, store, _) = open().await;
    let a = store.create_node(new_node(1)).await.unwrap().id;
    let b = store.create_node(new_node(2)).await.unwrap().id;
    let ground = store.create_floor(fields("Ground", 0), 32).await.unwrap();
    let upstairs = store.create_floor(fields("Upstairs", 1), 32).await.unwrap();

    store
        .place_node(ground.id, placed(a, 1.0, 2.0))
        .await
        .unwrap();
    store
        .place_node(ground.id, placed(b, 3.0, 4.0))
        .await
        .unwrap();
    let moved = store
        .place_node(upstairs.id, placed(a, 5.0, 6.0))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(moved.nodes, [placed(a, 5.0, 6.0)]);
    let floors = store.floors().await.unwrap();
    assert_eq!(floors[0].nodes, [placed(b, 3.0, 4.0)]);

    let missing = store
        .place_node(ground.id, placed(NodeId::new(99), 0.0, 0.0))
        .await
        .unwrap_err();
    assert!(matches!(missing, Error::NotFound { what: "node", .. }));
    assert_eq!(
        store
            .place_node(FloorId::new(99), placed(a, 0.0, 0.0))
            .await
            .unwrap(),
        None
    );

    assert!(store.unplace_node(b).await.unwrap());
    assert!(!store.unplace_node(b).await.unwrap());
    store.delete_node(a).await.unwrap();
    let upstairs = store.floor(upstairs.id).await.unwrap().unwrap();
    assert_eq!(upstairs.nodes, []);
    store
        .place_node(ground.id, placed(b, 3.0, 4.0))
        .await
        .unwrap();
    store.delete_floor(ground.id).await.unwrap();
    assert!(!store.unplace_node(b).await.unwrap());
}
