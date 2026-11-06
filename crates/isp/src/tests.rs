//! Every embedded preset: it parses, its id is its file name, its prompts are what it needs,
//! and filled in it is a valid site WAN.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;

use uniwrt_model::site::{
    SecretRef, SiteConfig,
    gateway::{WanIpv6, WanProto},
    validate::{Severity, validate},
};

use crate::{
    Answers, Ipv4Kind, Ipv6Kind, Preset, Prompt, SixRd, Step, files::FILES, hint, preset, presets,
};

fn answers() -> Answers {
    Answers {
        username: Some("user@isp".to_owned()),
        password: SecretRef::new(format!("config/{}", "a".repeat(32))),
        address: "203.0.113.10/29".parse().ok(),
        gateway: "203.0.113.9".parse().ok(),
        dns: vec!["9.9.9.9".parse().unwrap()],
        vlan: Some(7),
        mac: "02:00:00:00:00:01".parse().ok(),
        aftr: None,
        sixrd: Some(SixRd {
            prefix: "2001:db8::/32".parse().unwrap(),
            border_relay: "192.0.2.1".parse().unwrap(),
            ipv4_mask_len: 0,
        }),
    }
}

/// The prompts `preset` must ask for.
fn needed(preset: &Preset) -> BTreeSet<Prompt> {
    let mut needed = BTreeSet::new();
    match preset.ipv4.proto {
        Ipv4Kind::Pppoe => needed.extend([Prompt::Username, Prompt::Password]),
        Ipv4Kind::Static => needed.extend([Prompt::Address, Prompt::Gateway, Prompt::Dns]),
        _ => {}
    }
    if preset.ipv6.mode == Ipv6Kind::SixRd && preset.ipv6.sixrd.is_none() {
        needed.insert(Prompt::Sixrd);
    }
    needed
}

#[test]
fn every_preset_parses_and_is_named_by_its_file() {
    assert!(FILES.len() >= 9);
    for (stem, text) in FILES {
        let preset: Preset = toml::from_str(text).unwrap_or_else(|e| panic!("{stem}: {e}"));
        assert_eq!(&preset.id, stem);
        let prefix = preset
            .country
            .as_deref()
            .map_or("generic".to_owned(), str::to_lowercase);
        assert!(
            stem.starts_with(&format!("{prefix}-")),
            "{stem}: named after its country"
        );
        if let Some(country) = &preset.country {
            assert!(
                country.len() == 2 && country.bytes().all(|b| b.is_ascii_uppercase()),
                "{stem}: ISO 3166-1 alpha-2"
            );
            assert!(
                !preset.sources.is_empty(),
                "{stem}: an ISP's preset cites sources"
            );
        }
    }
    assert_eq!(presets().len(), FILES.len());
}

#[test]
fn every_preset_asks_what_it_needs_and_fills_a_valid_wan() {
    for preset in presets() {
        let id = &preset.id;
        let prompts: BTreeSet<Prompt> = preset.prompts.iter().copied().collect();
        assert_eq!(
            prompts.len(),
            preset.prompts.len(),
            "{id}: each prompt once"
        );
        assert!(prompts.is_superset(&needed(preset)), "{id}: {prompts:?}");
        if preset.ipv6.mode == Ipv6Kind::Auto {
            assert!(preset.fallback().is_some(), "{id}: auto falls back to 6rd");
        }
        let mut given = answers();
        if !prompts.contains(&Prompt::Vlan) {
            given.vlan = None;
        }
        if !prompts.contains(&Prompt::Mac) {
            given.mac = None;
        }
        let wan = preset.wan(&given).unwrap();
        assert_eq!(wan.preset.as_deref(), Some(id.as_str()));
        let mut site = SiteConfig::defaults(None);
        site.wan = wan;
        let errors: Vec<_> = validate(&site, &BTreeSet::new())
            .into_iter()
            .filter(|i| i.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{id}: {errors:?}");
        if let Some(fallback) = preset.fallback() {
            site.wan.ipv6 = fallback;
            assert!(
                validate(&site, &BTreeSet::new()).is_empty(),
                "{id} with 6rd"
            );
        }
    }
}

#[test]
fn a_missing_answer_names_its_prompt() {
    let pppoe = preset("generic-pppoe").unwrap();
    assert_eq!(pppoe.wan(&Answers::default()), Err(Prompt::Username));
    let static_ = preset("generic-static").unwrap();
    assert_eq!(static_.wan(&Answers::default()), Err(Prompt::Address));
    let sixrd = preset("generic-6rd").unwrap();
    assert_eq!(sixrd.wan(&Answers::default()), Err(Prompt::Sixrd));
}

#[test]
fn lumen_is_vlan_201_with_native_ipv6_then_6rd() {
    let lumen = preset("us-lumen-ipoe").unwrap();
    let wan = lumen.wan(&Answers::default()).unwrap();
    assert_eq!(wan.vlan, Some(201));
    assert!(matches!(wan.proto, WanProto::Dhcp(_)));
    assert_eq!(wan.ipv6, WanIpv6::Dhcpv6 { reqprefix: None });
    assert_eq!(
        lumen.fallback(),
        Some(WanIpv6::SixRd {
            prefix: "2602::/24".parse().unwrap(),
            border_relay: "205.171.2.64".parse().unwrap(),
            ipv4_mask_len: 0,
        })
    );
}

#[test]
fn presets_list_generic_ones_first() {
    let first_isp = presets().iter().position(|p| p.country.is_some()).unwrap();
    assert!(presets()[..first_isp].iter().all(|p| p.country.is_none()));
    assert!(presets()[first_isp..].iter().all(|p| p.country.is_some()));
}

#[test]
fn hints_prefer_the_presets_then_read_the_settings() {
    let lumen = preset("us-lumen-ipoe").unwrap();
    let wan = lumen.wan(&Answers::default()).unwrap();
    assert!(hint(Step::Address, &wan, Some(lumen), &[]).contains("transparent bridge"));
    assert!(hint(Step::Gateway, &wan, Some(lumen), &[]).contains("gateway"));
    let pppoe = preset("generic-pppoe").unwrap().wan(&answers()).unwrap();
    let refused = hint(Step::Address, &pppoe, None, &["AUTH_FAILED".to_owned()]);
    assert!(refused.contains("refused"));
    assert!(hint(Step::Address, &pppoe, None, &[]).contains("No PPPoE server"));
    let generic = preset("generic-dhcp-vlan")
        .unwrap()
        .wan(&answers())
        .unwrap();
    assert!(hint(Step::Address, &generic, None, &[]).contains("on VLAN 7"));
    for step in Step::ALL {
        assert!(hint(step, &generic, None, &[]).ends_with(['.', '?']));
    }
}
