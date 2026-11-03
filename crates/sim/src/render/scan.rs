//! The `scan` op's answer in the agent's raw shape (`node-agent/scan.uc`): the networks next door
//! (a fixed set outside the house), the other nodes' access points and mesh points (an unadopted
//! one too), each at the signal the path between them leaves, with their information elements;
//! and the channel survey of the band, busier where networks crowd.

use data_encoding::BASE64;
use serde_json::{Value, json};

use crate::{
    rng::Rng,
    world::{World, distance, radio::Band},
};

use super::wifi::{MESH_ID, SSID};

/// A network next door.
struct Foreign {
    ssid: Option<&'static str>,
    bssid: [u8; 6],
    band: Band,
    channel: u8,
    width: u32,
    pos: (f64, f64),
    txpower: f64,
    /// RSN AKM suite types; none for an open network.
    akms: &'static [u8],
    he: bool,
    /// Stations and channel utilization (of 255) it advertises.
    load: Option<(u16, u8)>,
}

#[allow(clippy::too_many_arguments)]
const fn foreign(
    ssid: Option<&'static str>,
    bssid: [u8; 6],
    (band, channel, width): (Band, u8, u32),
    pos: (f64, f64),
    txpower: f64,
    akms: &'static [u8],
    he: bool,
    load: Option<(u16, u8)>,
) -> Foreign {
    Foreign {
        ssid,
        bssid,
        band,
        channel,
        width,
        pos,
        txpower,
        akms,
        he,
        load,
    }
}

const NEIGHBORS: [Foreign; 11] = [
    foreign(
        Some("HOME-4F21"),
        [0x3c, 0x84, 0x6a, 0x4f, 0x21, 0x10],
        (Band::G2, 6, 20),
        (-9.0, 5.0),
        20.0,
        &[2],
        false,
        Some((5, 90)),
    ),
    foreign(
        Some("Apartment 2B"),
        [0x50, 0xd4, 0xf7, 0x12, 0x9a, 0x02],
        (Band::G2, 1, 40),
        (12.0, -10.0),
        20.0,
        &[2, 8],
        true,
        None,
    ),
    foreign(
        Some("DIRECT-7A-Printer"),
        [0x9c, 0x2b, 0x0c, 0x7a, 0x55, 0x01],
        (Band::G2, 11, 20),
        (30.0, 22.0),
        14.0,
        &[2],
        false,
        None,
    ),
    foreign(
        Some("Pixel_9821"),
        [0x42, 0x1f, 0x9e, 0x98, 0x21, 0x0e],
        (Band::G2, 6, 20),
        (-4.0, -7.0),
        12.0,
        &[8],
        true,
        Some((1, 20)),
    ),
    foreign(
        None,
        [0x08, 0xbf, 0xb8, 0x33, 0x01, 0x7c],
        (Band::G2, 11, 20),
        (18.0, -5.0),
        18.0,
        &[2],
        false,
        None,
    ),
    foreign(
        Some("Smith Family"),
        [0xe4, 0x5f, 0x01, 0x6c, 0x0d, 0x31],
        (Band::G2, 3, 20),
        (40.0, 8.0),
        20.0,
        &[2],
        false,
        None,
    ),
    foreign(
        Some("HOME-4F21"),
        [0x3c, 0x84, 0x6a, 0x4f, 0x21, 0x11],
        (Band::G5, 44, 80),
        (-9.0, 5.0),
        23.0,
        &[2, 8],
        true,
        Some((3, 40)),
    ),
    foreign(
        Some("Apartment 2B"),
        [0x50, 0xd4, 0xf7, 0x12, 0x9a, 0x03],
        (Band::G5, 149, 80),
        (12.0, -10.0),
        23.0,
        &[2, 8],
        true,
        None,
    ),
    foreign(
        Some("Corner Cafe"),
        [0x70, 0xa7, 0x41, 0x2c, 0x77, 0x20],
        (Band::G5, 100, 80),
        (35.0, 25.0),
        23.0,
        &[],
        false,
        Some((14, 140)),
    ),
    foreign(
        Some("Smith Family"),
        [0xe4, 0x5f, 0x01, 0x6c, 0x0d, 0x32],
        (Band::G5, 40, 40),
        (40.0, 8.0),
        20.0,
        &[2],
        false,
        None,
    ),
    foreign(
        Some("Office-Guest"),
        [0x24, 0x5a, 0x4c, 0x90, 0x10, 0x05],
        (Band::G5, 52, 20),
        (32.0, -4.0),
        20.0,
        &[18],
        true,
        None,
    ),
];

/// Exterior walls between the house and the neighbors.
const WALLS_DB: f64 = 12.0;
/// What the radio cannot hear below.
const SENSITIVITY_DBM: f64 = -92.0;

const CHANNELS_2G: [u8; 11] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const CHANNELS_5G: [u8; 25] = [
    36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144,
    149, 153, 157, 161, 165,
];

/// The scan of node `n`'s radio behind `ifname`, or `None` for an interface it lacks.
pub(crate) fn scan(world: &World, n: usize, ifname: &str) -> Option<Value> {
    let band = match ifname {
        "phy0-ap0" => Band::G2,
        "phy1-ap0" | "phy1-mesh0" => Band::G5,
        _ => return None,
    };
    let node = &world.nodes[n];
    let mut rng = Rng::new(world.now_s ^ ((n as u64) << 40) ^ (band.index() as u64));
    let mut bss = Vec::new();
    // (frequency, width, center, signal) of each network heard, for the survey.
    let mut heard = Vec::new();
    for other in world
        .nodes
        .iter()
        .enumerate()
        .filter(|(m, o)| *m != n && o.up())
    {
        let (_, other) = other;
        let radio = &other.radios[band.index()];
        let signal = f64::from(radio.txpower) + 3.0 - band.path_loss(distance(node.pos, other.pos))
            + rng.normal() * 1.5;
        if signal < SENSITIVITY_DBM {
            continue;
        }
        let (width, center) = (band.width_mhz(), band.center_freq(radio.channel));
        let mut elements = ies(
            Some(SSID),
            None,
            band,
            radio.channel,
            width,
            &[2, 8],
            true,
            None,
        );
        heard.push((band.freq(radio.channel), width, center, signal));
        bss.push(entry(
            &other.bssid(band).to_string(),
            band.freq(radio.channel),
            signal,
            &elements,
        ));
        if band == Band::G5 {
            elements = ies(
                None,
                Some(MESH_ID),
                band,
                radio.channel,
                width,
                &[8],
                true,
                None,
            );
            bss.push(entry(
                &other.mesh_mac().to_string(),
                band.freq(radio.channel),
                signal + 1.0,
                &elements,
            ));
        }
    }
    for net in NEIGHBORS.iter().filter(|net| net.band == band) {
        let signal = net.txpower - band.path_loss(distance(node.pos, net.pos)) - WALLS_DB
            + rng.normal() * 2.0;
        if signal < SENSITIVITY_DBM {
            continue;
        }
        let center = center(band, net.channel, net.width);
        heard.push((band.freq(net.channel), net.width, center, signal));
        let elements = ies(
            net.ssid,
            None,
            band,
            net.channel,
            net.width,
            net.akms,
            net.he,
            net.load,
        );
        let bssid = net.bssid.map(|b| format!("{b:02x}")).join(":");
        bss.push(entry(&bssid, band.freq(net.channel), signal, &elements));
    }
    let survey = survey(
        band,
        node.radios[band.index()].busy,
        node.radios[band.index()].channel,
        &heard,
        &mut rng,
    );
    let started = world.now_ms();
    let duration = match band {
        Band::G2 => 2_600,
        Band::G5 => 4_800,
    };
    Some(json!({
        "started_ms": started,
        "ended_ms": started + duration,
        "bss": bss,
        "survey_before": [],
        "survey_after": survey,
    }))
}

fn entry(bssid: &str, frequency: u32, signal: f64, elements: &[u8]) -> Value {
    json!({
        "bssid": bssid,
        "frequency": frequency,
        "signal_mbm": (signal.round() as i64) * 100,
        "capability": 0x0411,
        "seen_ms_ago": 400,
        "ies": BASE64.encode(elements),
    })
}

/// Whether a 40 MHz channel's secondary half lies above its primary.
fn above(band: Band, channel: u8) -> bool {
    match band {
        Band::G2 => channel <= 7,
        Band::G5 => (channel / 4) % 2 == 1,
    }
}

/// The center of a `width` channel whose primary is `channel`.
fn center(band: Band, channel: u8, width: u32) -> u32 {
    match width {
        80 if band == Band::G5 => band.center_freq(channel),
        40 if above(band, channel) => band.freq(channel) + 10,
        40 => band.freq(channel) - 10,
        _ => band.freq(channel),
    }
}

fn ie(out: &mut Vec<u8>, id: u8, body: &[u8]) {
    out.push(id);
    out.push(u8::try_from(body.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(body);
}

/// The elements a beacon of this network carries.
#[allow(clippy::too_many_arguments)]
fn ies(
    ssid: Option<&str>,
    mesh_id: Option<&str>,
    band: Band,
    channel: u8,
    width: u32,
    akms: &[u8],
    he: bool,
    load: Option<(u16, u8)>,
) -> Vec<u8> {
    let mut out = Vec::new();
    ie(&mut out, 0, ssid.unwrap_or_default().as_bytes());
    if band == Band::G2 {
        ie(&mut out, 3, &[channel]);
    }
    if let Some((stations, utilization)) = load {
        let [s0, s1] = stations.to_le_bytes();
        ie(&mut out, 11, &[s0, s1, utilization, 0, 0]);
    }
    ie(&mut out, 45, &[0; 26]);
    let secondary = match width {
        20 => 0x00,
        _ if above(band, channel) => 0x05,
        _ => 0x07,
    };
    let mut ht = vec![channel, secondary];
    ht.resize(22, 0);
    ie(&mut out, 61, &ht);
    if band == Band::G5 {
        ie(&mut out, 191, &[0; 12]);
        let vht = if width == 80 {
            let ccfs0 = u8::try_from((band.center_freq(channel) - 5000) / 5).unwrap_or(0);
            [1, ccfs0, 0, 0, 0]
        } else {
            [0; 5]
        };
        ie(&mut out, 192, &vht);
    }
    if let Some(mesh_id) = mesh_id {
        ie(&mut out, 114, mesh_id.as_bytes());
    }
    if he {
        ie(&mut out, 255, &[35, 0, 0, 0, 0, 0, 0]);
    }
    if !akms.is_empty() {
        let mut rsn = vec![1, 0, 0x00, 0x0f, 0xac, 4, 1, 0, 0x00, 0x0f, 0xac, 4];
        rsn.extend_from_slice(&u16::try_from(akms.len()).unwrap_or(0).to_le_bytes());
        for &akm in akms {
            rsn.extend_from_slice(&[0x00, 0x0f, 0xac, akm]);
        }
        ie(&mut out, 48, &rsn);
    }
    out
}

/// The survey of every channel on `band`: busy from the networks `heard` (frequency, width,
/// center, signal) that overlap it, plus the radio's own load (`own_busy`) on `own_channel`.
fn survey(
    band: Band,
    own_busy: f64,
    own_channel: u8,
    heard: &[(u32, u32, u32, f64)],
    rng: &mut Rng,
) -> Vec<Value> {
    let channels: &[u8] = match band {
        Band::G2 => &CHANNELS_2G,
        Band::G5 => &CHANNELS_5G,
    };
    channels
        .iter()
        .map(|&channel| {
            let freq = band.freq(channel);
            let crowd: f64 = heard
                .iter()
                .filter(|(_, width, center, _)| freq.abs_diff(*center) < width / 2 + 10)
                .map(|(.., signal)| ((signal + 95.0) / 3.0).clamp(0.0, 15.0))
                .sum();
            let busy = if channel == own_channel {
                own_busy
            } else {
                2.0
            } + crowd;
            let noise = if band == Band::G2 { -91.0 } else { -95.0 } + rng.normal();
            json!({
                "frequency": freq,
                "noise": noise.round() as i64,
                "time": 110,
                "busy": (busy.min(95.0) * 1.1).round() as u64,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use uniwrt_core::Timestamp;
    use uniwrt_model::{rf::Security, wifi::Band as WifiBand, wifi::ChannelWidth};
    use uniwrt_node::{facts::scan::heard, proto::ops::Scanned};

    use super::scan;
    use crate::{SimConfig, world::World};

    fn heard_by(world: &World, n: usize, ifname: &str, band: WifiBand) -> uniwrt_model::rf::Heard {
        let raw: Scanned = serde_json::from_value(scan(world, n, ifname).unwrap()).unwrap();
        heard(&raw, band)
    }

    #[test]
    fn the_gateway_hears_the_other_nodes_and_the_neighbors() {
        let world = World::new(
            &SimConfig::default(),
            Timestamp::from_unix_ms(1_793_245_539_000),
        );
        let five = heard_by(&world, 0, "phy1-ap0", WifiBand::G5);
        let own: Vec<_> = five
            .bsss
            .iter()
            .filter(|b| b.ssid.as_deref() == Some(super::SSID))
            .collect();
        assert_eq!(own.len(), world.nodes.len() - 1, "{:?}", five.bsss);
        assert!(
            own.iter()
                .all(|b| b.width == ChannelWidth::Mhz80 && b.center_frequency_mhz == 5210)
        );
        assert!(own.iter().all(|b| b.security == Security::Wpa2Wpa3));
        assert!(
            five.bsss
                .iter()
                .any(|b| b.mesh_id.as_deref() == Some(super::MESH_ID))
        );
        let cafe = five
            .bsss
            .iter()
            .find(|b| b.ssid.as_deref() == Some("Corner Cafe"));
        assert!(cafe.is_none_or(|c| c.security == Security::Open && c.channel == 100));
        let smith = five
            .bsss
            .iter()
            .find(|b| b.ssid.as_deref() == Some("Smith Family"));
        assert!(
            smith.is_none_or(|s| s.width == ChannelWidth::Mhz40 && s.center_frequency_mhz == 5190)
        );
        assert_eq!(five.channels.len(), 25);
        let two = heard_by(&world, 0, "phy0-ap0", WifiBand::G2);
        let home = two
            .bsss
            .iter()
            .find(|b| b.ssid.as_deref() == Some("HOME-4F21"))
            .unwrap();
        assert_eq!((home.channel, home.stations), (6, Some(5)));
        assert!(two.bsss.iter().any(|b| b.ssid.is_none()), "the hidden one");
        assert!(scan(&world, 0, "phy9-ap0").is_none());
    }
}
