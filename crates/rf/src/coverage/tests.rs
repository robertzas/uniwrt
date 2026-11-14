use uniwrt_core::{FloorId, NodeId, Timestamp};
use uniwrt_model::{
    floor::{Floor, FloorScale, LengthUnit, Material, Placement, Point, Wall},
    wifi::Band,
};

use super::{
    BandModel, Transmitter, coverage,
    geometry::{Located, elevation},
    walls::{crosses, storeys, wall_loss},
};
use crate::{BandReport, MatrixEntry, MatrixSource, RadioReport, RfReport};

/// A floor at `level`, 1 px = 0.1 m, with nodes placed at points given in meters.
fn floor(level: i16, height_m: f64, nodes: &[(i64, f64, f64)]) -> Floor {
    Floor {
        id: FloorId::new(i64::from(level) + 100),
        name: format!("Level {level}"),
        level,
        height_m,
        image: None,
        scale: Some(FloorScale {
            from: Point { x: 0.0, y: 0.0 },
            to: Point { x: 100.0, y: 0.0 },
            length_m: 10.0,
            unit: LengthUnit::M,
        }),
        nodes: nodes
            .iter()
            .map(|&(node, x, y)| Placement {
                node: NodeId::new(node),
                at: Point {
                    x: x * 10.0,
                    y: y * 10.0,
                },
            })
            .collect(),
        walls: Vec::new(),
        updated_at: Timestamp::UNIX_EPOCH,
    }
}

fn radio(node: i64, up: bool, tx: Option<i16>) -> RadioReport {
    RadioReport {
        node: NodeId::new(node),
        radio: format!("radio{node}"),
        up,
        channel: Some(36),
        frequency_mhz: Some(5180),
        center_frequency_mhz: Some(5210),
        width: None,
        tx_power_dbm: tx,
        busy_percent: None,
        noise_dbm: None,
        on_channel: Vec::new(),
        interference: 0.0,
    }
}

fn entry(from: i64, to: i64, signal_dbm: i16) -> MatrixEntry {
    MatrixEntry {
        from: NodeId::new(from),
        to: NodeId::new(to),
        signal_dbm,
        source: MatrixSource::Scan,
        at: Timestamp::UNIX_EPOCH,
    }
}

fn report(band: Band, radios: Vec<RadioReport>, matrix: Vec<MatrixEntry>) -> RfReport {
    RfReport {
        bands: vec![BandReport {
            band,
            radios,
            channels: Vec::new(),
            networks: Vec::new(),
            matrix,
        }],
    }
}

/// The signal a model with exponent `n` and floor loss `f` predicts at 5 GHz from 20 dBm.
#[allow(clippy::cast_possible_truncation)]
fn heard(n: f64, f: f64, d: f64, floors: u16) -> i16 {
    (20.0 - 47.3 - 10.0 * n * d.log10() - f * f64::from(floors)).round() as i16
}

fn only(coverage: &super::Coverage) -> &BandModel {
    assert_eq!(coverage.bands.len(), 1);
    &coverage.bands[0]
}

#[test]
fn the_exponent_is_fitted_to_same_floor_measurements() {
    let floors = [floor(
        0,
        3.0,
        &[(1, 0.0, 0.0), (2, 8.0, 0.0), (3, 0.0, 15.0)],
    )];
    let d23 = 8.0_f64.hypot(15.0);
    let matrix = vec![
        entry(1, 2, heard(3.6, 0.0, 8.0, 0)),
        entry(2, 1, heard(3.6, 0.0, 8.0, 0)),
        entry(1, 3, heard(3.6, 0.0, 15.0, 0)),
        entry(2, 3, heard(3.6, 0.0, d23, 0)),
    ];
    let radios = (1..=3).map(|n| radio(n, true, Some(20))).collect();
    let coverage = coverage(&floors, &report(Band::G5, radios, matrix));
    let model = only(&coverage);
    assert!((model.exponent - 3.6).abs() < 0.05, "{model:?}");
    assert!(model.exponent_fitted);
    assert!(!model.floor_loss_fitted);
    assert!((model.floor_loss_db - 16.0).abs() < f32::EPSILON);
    assert_eq!(model.samples, 4);
    assert!(model.error_db.is_some_and(|e| e < 0.5), "{model:?}");
    assert!((model.loss_at_1m_db - 47.3).abs() < f32::EPSILON);
}

#[test]
fn the_floor_loss_is_fitted_to_measurements_between_floors() {
    // 1 and 3 share the ground floor; 2 is right above 1, 3.5 m up.
    let floors = [
        floor(0, 3.5, &[(1, 0.0, 0.0), (3, 12.0, 0.0)]),
        floor(1, 3.0, &[(2, 0.0, 0.0)]),
    ];
    let d23 = 12.0_f64.hypot(3.5);
    let matrix = vec![
        entry(1, 3, heard(3.0, 14.0, 12.0, 0)),
        entry(3, 1, heard(3.0, 14.0, 12.0, 0)),
        entry(1, 2, heard(3.0, 14.0, 3.5, 1)),
        entry(3, 2, heard(3.0, 14.0, d23, 1)),
    ];
    let radios = (1..=3).map(|n| radio(n, true, Some(20))).collect();
    let coverage = coverage(&floors, &report(Band::G5, radios, matrix));
    let model = only(&coverage);
    assert!((model.exponent - 3.0).abs() < 0.05, "{model:?}");
    assert!((model.floor_loss_db - 14.0).abs() < 0.6, "{model:?}");
    assert!(model.floor_loss_fitted);
}

#[test]
fn without_measurements_the_band_keeps_its_defaults() {
    // 2 is on a floor without a scale and 3 isn't placed: neither has meters.
    let mut unscaled = floor(1, 3.0, &[(2, 1.0, 1.0)]);
    unscaled.scale = None;
    let floors = [floor(0, 3.0, &[(1, 0.0, 0.0)]), unscaled];
    let matrix = vec![entry(1, 2, -50), entry(1, 3, -50), entry(2, 1, -50)];
    let radios = vec![
        radio(1, true, Some(17)),
        radio(2, true, None),
        radio(3, true, Some(23)),
    ];
    let coverage = coverage(&floors, &report(Band::G2, radios, matrix));
    let model = only(&coverage);
    assert_eq!(
        (model.exponent, model.floor_loss_db, model.samples),
        (3.0, 12.0, 0)
    );
    assert!(!model.exponent_fitted && !model.floor_loss_fitted);
    assert_eq!(model.error_db, None);
    assert!((model.loss_at_1m_db - 40.2).abs() < f32::EPSILON);
    let tx: Vec<(i64, i16)> = model
        .transmitters
        .iter()
        .map(|t| (t.node.get(), t.tx_power_dbm))
        .collect();
    assert_eq!(tx, [(1, 17), (2, 20), (3, 23)]);
}

#[test]
fn a_node_sends_with_its_strongest_radio_that_is_up() {
    let radios = vec![
        radio(1, true, Some(14)),
        radio(1, true, Some(21)),
        radio(1, false, Some(26)),
        radio(2, false, Some(20)),
    ];
    let coverage = coverage(&[], &report(Band::G5, radios, Vec::new()));
    assert_eq!(
        only(&coverage).transmitters,
        [Transmitter {
            node: NodeId::new(1),
            tx_power_dbm: 21
        }]
    );
}

#[test]
fn fits_stay_within_their_bounds() {
    let floors = [
        floor(0, 3.0, &[(1, 0.0, 0.0), (2, 5.0, 0.0)]),
        floor(2, 3.0, &[(3, 0.0, 0.0)]),
    ];
    // Far too weak on the floor and far too strong across two of them.
    let matrix = vec![entry(1, 2, -120), entry(1, 3, 10)];
    let radios = (1..=3).map(|n| radio(n, true, Some(20))).collect();
    let coverage = coverage(&floors, &report(Band::G5, radios, matrix));
    let model = only(&coverage);
    assert!((model.exponent - 6.0).abs() < f32::EPSILON);
    assert!(model.floor_loss_db.abs() < f32::EPSILON);
}

#[test]
fn sixty_ghz_is_not_modeled() {
    let coverage = coverage(
        &[],
        &report(Band::G60, vec![radio(1, true, None)], Vec::new()),
    );
    assert_eq!(coverage.bands, []);
}

#[test]
fn floors_stack_by_level_with_missing_ones_at_the_default_height() {
    let floors = [floor(-1, 2.5, &[]), floor(0, 4.0, &[]), floor(2, 3.5, &[])];
    assert!((elevation(&floors, 0)).abs() < f64::EPSILON);
    assert!((elevation(&floors, 1) - 4.0).abs() < f64::EPSILON);
    assert!((elevation(&floors, 2) - 7.0).abs() < f64::EPSILON); // level 1 is missing: 3 m
    assert!((elevation(&floors, 3) - 10.5).abs() < f64::EPSILON);
    assert!((elevation(&floors, -1) + 2.5).abs() < f64::EPSILON);
    assert!((elevation(&floors, -2) + 5.5).abs() < f64::EPSILON);
}

/// A wall given in meters (1 px = 0.1 m).
fn wall(from: (f64, f64), to: (f64, f64), material: Material) -> Wall {
    Wall {
        from: Point {
            x: from.0 * 10.0,
            y: from.1 * 10.0,
        },
        to: Point {
            x: to.0 * 10.0,
            y: to.1 * 10.0,
        },
        material,
    }
}

#[test]
fn walls_are_taken_off_before_the_fit() {
    // A concrete wall at x = 5 m between 1 and 2, and between 2 and 3.
    let mut ground = floor(0, 3.0, &[(1, 0.0, 0.0), (2, 10.0, 0.0), (3, 0.0, 12.0)]);
    ground.walls = vec![wall((5.0, -5.0), (5.0, 20.0), Material::Concrete)];
    let d23 = 10.0_f64.hypot(12.0);
    let concrete = Material::Concrete.loss_db(Band::G5);
    #[allow(clippy::cast_possible_truncation)]
    let through = |d: f64| (f64::from(heard(3.2, 0.0, d, 0)) - concrete).round() as i16;
    let matrix = vec![
        entry(1, 2, through(10.0)),
        entry(1, 3, heard(3.2, 0.0, 12.0, 0)),
        entry(2, 3, through(d23)),
    ];
    let radios: Vec<RadioReport> = (1..=3).map(|n| radio(n, true, Some(20))).collect();
    let walled = coverage(
        std::slice::from_ref(&ground),
        &report(Band::G5, radios.clone(), matrix.clone()),
    );
    let model = only(&walled);
    assert!((model.exponent - 3.2).abs() < 0.05, "{model:?}");
    assert!(model.error_db.is_some_and(|e| e < 0.5), "{model:?}");
    assert_eq!(model.wall_losses.len(), Material::ALL.len());
    assert!(model.wall_losses.iter().any(
        |w| w.material == Material::Concrete && (f64::from(w.loss_db) - concrete).abs() < 1e-6
    ));

    // Without the wall the exponent soaks up its loss.
    ground.walls.clear();
    let bare = coverage(&[ground], &report(Band::G5, radios, matrix));
    assert!(only(&bare).exponent > 3.8, "{:?}", only(&bare));
}

#[test]
fn a_path_between_floors_crosses_each_storeys_walls_on_its_part() {
    // A brick wall at x = 8 m on the ground floor, a metal one at x = 2 m upstairs; the path rises
    // from 1 m above the ground floor (z 1) to 1 m above the first (z 4) over x 0 → 12 m, so it
    // leaves the ground storey at z 3, x = 8 m: the brick wall is at its very end, the metal one
    // isn't on the upper part at all.
    let mut ground = floor(0, 3.0, &[]);
    ground.walls = vec![wall((8.0, -1.0), (8.0, 1.0), Material::Brick)];
    let mut first = floor(1, 3.0, &[]);
    first.walls = vec![
        wall((2.0, -1.0), (2.0, 1.0), Material::Metal),
        wall((10.0, -1.0), (10.0, 1.0), Material::Glass),
    ];
    let floors = [ground, first];
    let list = storeys(&floors);
    let at = |level: i16, x: f64, z: f64| Located {
        node: NodeId::new(1),
        level,
        x,
        y: 0.0,
        z,
    };
    let (low, high) = (at(0, 0.0, 0.0), at(1, 12.0, 3.0));
    let expected = Material::Brick.loss_db(Band::G5) + Material::Glass.loss_db(Band::G5);
    assert!((wall_loss(&list, &low, &high, Band::G5) - expected).abs() < 1e-9);
    assert!((wall_loss(&list, &high, &low, Band::G5) - expected).abs() < 1e-9);
    // Within the first floor, only its walls count.
    let across = wall_loss(&list, &at(1, 0.0, 3.0), &at(1, 12.0, 3.0), Band::G2);
    let both = Material::Metal.loss_db(Band::G2) + Material::Glass.loss_db(Band::G2);
    assert!((across - both).abs() < 1e-9);
    assert!(!crosses((0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)));
    assert!(crosses((0.0, 0.0), (2.0, 0.0), (1.0, -1.0), (1.0, 0.0)));
}
