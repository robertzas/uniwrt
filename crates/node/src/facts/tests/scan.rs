use data_encoding::BASE64;
use uniwrt_model::{
    rf::{ChannelSurvey, Security},
    wifi::{Band, ChannelWidth, WifiStandard},
};

use super::heard;
use crate::proto::ops::{RawBss, RawSurvey, Scanned};

fn ie(id: u8, body: &[u8]) -> Vec<u8> {
    let mut bytes = vec![id, u8::try_from(body.len()).unwrap()];
    bytes.extend_from_slice(body);
    bytes
}

/// An RSN element with CCMP and these AKM suite types.
fn rsn(akms: &[u8]) -> Vec<u8> {
    let mut body = vec![1, 0, 0x00, 0x0f, 0xac, 4, 1, 0, 0x00, 0x0f, 0xac, 4];
    body.extend_from_slice(&u16::try_from(akms.len()).unwrap().to_le_bytes());
    for &akm in akms {
        body.extend_from_slice(&[0x00, 0x0f, 0xac, akm]);
    }
    ie(48, &body)
}

fn bss(bssid: &str, frequency: u32, signal_dbm: i32, elements: &[Vec<u8>]) -> RawBss {
    RawBss {
        bssid: bssid.to_owned(),
        frequency,
        signal_mbm: Some(signal_dbm * 100),
        capability: Some(0x0401),
        seen_ms_ago: Some(200),
        ies: Some(BASE64.encode(&elements.concat())),
    }
}

fn scanned(bss: Vec<RawBss>) -> Scanned {
    Scanned {
        started_ms: 10_000,
        ended_ms: 13_500,
        bss,
        ..Scanned::default()
    }
}

#[test]
fn a_vht80_network_names_its_whole_channel_security_and_load() {
    let raw = scanned(vec![bss(
        "02:11:22:33:44:55",
        5180,
        -61,
        &[
            ie(0, b"Neighbor"),
            ie(45, &[0; 26]),
            ie(61, &[36, 0x05, 0, 0, 0]),
            ie(191, &[0; 12]),
            ie(192, &[1, 42, 0, 0, 0]),
            rsn(&[2, 8]),
            ie(11, &[3, 0, 128, 0, 0]),
        ],
    )]);
    let heard = heard(&raw, Band::G5);
    assert_eq!(heard.duration_ms, 3_500);
    let [bss] = heard.bsss.as_slice() else {
        panic!("{:?}", heard.bsss)
    };
    assert_eq!(bss.ssid.as_deref(), Some("Neighbor"));
    assert_eq!((bss.channel, bss.frequency_mhz), (36, 5180));
    assert_eq!(
        (bss.width, bss.center_frequency_mhz),
        (ChannelWidth::Mhz80, 5210)
    );
    assert_eq!(bss.signal_dbm, -61);
    assert_eq!(bss.standard, WifiStandard::Vht);
    assert_eq!(bss.security, Security::Wpa2Wpa3);
    assert_eq!((bss.stations, bss.utilization_percent), (Some(3), Some(50)));
}

#[test]
fn a_2g_network_heard_on_a_neighbor_channel_is_placed_on_its_own() {
    let raw = scanned(vec![bss(
        "02:00:00:00:00:01",
        2457,
        -70,
        &[
            ie(0, &[0, 0, 0]),
            ie(3, &[11]),
            ie(61, &[11, 0x07, 0, 0, 0]),
        ],
    )]);
    let bss = &heard(&raw, Band::G2).bsss[0];
    assert_eq!(bss.ssid, None, "hidden");
    assert_eq!((bss.channel, bss.frequency_mhz), (11, 2462));
    assert_eq!(
        (bss.width, bss.center_frequency_mhz),
        (ChannelWidth::Mhz40, 2452)
    );
    assert_eq!(bss.standard, WifiStandard::Ht);
    assert_eq!(bss.security, Security::Open);
}

#[test]
fn six_gigahertz_widths_come_from_he_and_eht_operations() {
    // HE operation: parameters with the 6 GHz operation present (bit 17), color, basic MCS, then
    // primary 37, 160 MHz, CCFS0 39, CCFS1 47.
    let he = ie(255, &[36, 0, 0, 0x02, 1, 0xff, 0xfc, 37, 3, 39, 47, 0]);
    let raw = scanned(vec![
        bss(
            "02:00:00:00:00:02",
            6135,
            -55,
            &[ie(0, b"six"), he.clone(), rsn(&[8])],
        ),
        bss(
            "02:00:00:00:00:03",
            6135,
            -58,
            &[
                ie(0, b"seven"),
                he,
                ie(255, &[108, 0, 0]),
                ie(255, &[106, 1, 0, 0, 0, 0, 4, 31, 63]),
                rsn(&[24]),
            ],
        ),
    ]);
    let heard = heard(&raw, Band::G6);
    let he = &heard.bsss[0];
    assert_eq!(he.channel, 37);
    assert_eq!(
        (he.width, he.center_frequency_mhz),
        (ChannelWidth::Mhz160, 6185)
    );
    assert_eq!(
        (he.standard, he.security),
        (WifiStandard::He, Security::Wpa3)
    );
    let eht = &heard.bsss[1];
    assert_eq!(
        (eht.width, eht.center_frequency_mhz),
        (ChannelWidth::Mhz320, 6265)
    );
    assert_eq!(
        (eht.standard, eht.security),
        (WifiStandard::Eht, Security::Wpa3)
    );
}

#[test]
fn security_reads_akms_wpa_and_the_privacy_bit() {
    let wpa = ie(221, &[0x00, 0x50, 0xf2, 0x01, 1, 0]);
    let cases = [
        (vec![rsn(&[1])], 0x0411, Security::Enterprise),
        (vec![rsn(&[18])], 0x0411, Security::Owe),
        (vec![rsn(&[2])], 0x0411, Security::Wpa2),
        (vec![wpa], 0x0411, Security::Wpa),
        (vec![], 0x0411, Security::Wep),
        (vec![], 0x0401, Security::Open),
    ];
    for (i, (elements, capability, security)) in cases.into_iter().enumerate() {
        let mut raw = bss(&format!("02:00:00:00:01:{i:02x}"), 2412, -60, &elements);
        raw.capability = Some(capability);
        let heard = heard(&scanned(vec![raw]), Band::G2);
        assert_eq!(heard.bsss[0].security, security, "case {i}");
    }
}

#[test]
fn stale_duplicate_and_foreign_band_results_are_left_out() {
    let mut stale = bss("02:00:00:00:00:10", 5180, -40, &[ie(0, b"old")]);
    stale.seen_ms_ago = Some(60_000);
    let raw = scanned(vec![
        stale,
        bss("02:00:00:00:00:11", 5180, -80, &[ie(0, b"twice")]),
        bss("02:00:00:00:00:11", 5200, -66, &[ie(0, b"twice")]),
        bss("02:00:00:00:00:12", 2412, -50, &[ie(0, b"other band")]),
        // Malformed elements: an SSID whose length runs past the end.
        bss("02:00:00:00:00:13", 5745, -75, &[vec![0, 40, b'x']]),
    ]);
    let heard = heard(&raw, Band::G5);
    let found: Vec<_> = heard
        .bsss
        .iter()
        .map(|b| (b.bssid.to_string(), b.signal_dbm, b.ssid.clone()))
        .collect();
    assert_eq!(
        found,
        [
            (
                "02:00:00:00:00:11".to_owned(),
                -66,
                Some("twice".to_owned())
            ),
            ("02:00:00:00:00:13".to_owned(), -75, None),
        ]
    );
}

#[test]
fn the_survey_is_what_each_channel_did_while_the_scan_listened() {
    let survey = |frequency, time, busy| RawSurvey {
        frequency,
        noise: Some(-95),
        time: Some(time),
        busy: Some(busy),
    };
    let raw = Scanned {
        survey_before: vec![survey(5180, 1_000, 100), survey(5220, 500, 40)],
        survey_after: vec![
            survey(5180, 1_100, 150),
            survey(5200, 120, 12),
            survey(5220, 500, 40),
            survey(2412, 300, 30),
        ],
        ..scanned(vec![])
    };
    let channel = |channel, frequency_mhz, busy_percent, dwell_ms| ChannelSurvey {
        channel,
        frequency_mhz,
        noise_dbm: Some(-95),
        busy_percent,
        dwell_ms,
    };
    assert_eq!(
        heard(&raw, Band::G5).channels,
        [
            channel(36, 5180, Some(50.0), Some(100)),
            channel(40, 5200, Some(10.0), Some(120)),
            channel(44, 5220, None, None),
        ]
    );
}
