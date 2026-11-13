use uniwrt_model::{
    facts::{RadioFacts, WifiIfaceFacts, WifiIfaceMode},
    wifi::{Band, ChannelWidth},
};
use uniwrt_uci::Package;

use super::air;

fn wireless(radio1: &str, extra: &str) -> Package {
    let text = format!(
        "config wifi-device 'radio0'\n\toption band '2g'\n\toption channel '1'\n\toption htmode \
         'HE20'\n\nconfig wifi-device 'radio1'\n\toption band '5g'\n{radio1}\nconfig wifi-iface \
         'default_radio1'\n\toption device 'radio1'\n\toption mode 'ap'\n\toption ssid \
         'home'\n{extra}"
    );
    uniwrt_uci::parse("wireless", &text).unwrap()
}

fn radio(name: &str, band: Band, modes: &[WifiIfaceMode]) -> RadioFacts {
    RadioFacts {
        name: name.to_owned(),
        phy: Some(format!("phy{}", &name[5..])),
        path: None,
        band,
        up: true,
        disabled: false,
        channel: Some(36),
        frequency_mhz: Some(5180),
        center_frequency_mhz: Some(5210),
        width: Some(ChannelWidth::Mhz80),
        standard: None,
        tx_power_dbm: Some(20),
        noise_dbm: None,
        channel_busy_percent: None,
        country: None,
        ifaces: modes
            .iter()
            .enumerate()
            .map(|(i, mode)| WifiIfaceFacts {
                name: Some(format!("phy{}-ap{i}", &name[5..])),
                section: None,
                mode: *mode,
                ssid: None,
                mesh_id: None,
                bssid: None,
                encryption: None,
                networks: Vec::new(),
                up: true,
            })
            .collect(),
    }
}

fn radios() -> Vec<RadioFacts> {
    vec![
        radio("radio0", Band::G2, &[WifiIfaceMode::Ap]),
        radio("radio1", Band::G5, &[WifiIfaceMode::Ap]),
    ]
}

const AT_36: &str = "\toption channel '36'\n\toption htmode 'HE80'\n";

#[test]
fn a_channel_and_width_move_by_a_switch() {
    let before = wireless(AT_36, "");
    let after = wireless("\toption channel '149'\n\toption htmode 'HE40'\n", "");
    let air = air(&before, &after, &radios()).unwrap();
    assert_eq!(air.first, None);
    assert_eq!(air.wireless, after);
    assert!(air.live());
    let [m] = air.moves.as_slice() else {
        panic!("{:?}", air.moves)
    };
    assert_eq!(
        (m.radio.as_str(), m.channel, m.width),
        ("radio1", 149, ChannelWidth::Mhz40)
    );
    let (ifname, switch) = m.switch.as_ref().unwrap();
    assert_eq!(ifname, "phy1-ap0");
    assert_eq!(
        (switch.freq, switch.center_freq1, switch.bandwidth),
        (5745, 5755, 40)
    );
}

#[test]
fn power_goes_first_and_alone_is_no_move() {
    let before = wireless(AT_36, "");
    let powered = wireless(&format!("{AT_36}\toption txpower '17'\n"), "");
    assert_eq!(air(&before, &powered, &radios()), None);
    let after = wireless(
        "\toption channel '44'\n\toption htmode 'HE80'\n\toption txpower '17'\n",
        "",
    );
    let air = air(&before, &after, &radios()).unwrap();
    // The first transaction holds the power on the channel it runs.
    assert_eq!(air.first, Some(powered));
    assert_eq!(air.moves.len(), 1);
    assert_eq!(air.moves[0].channel, 44);
}

#[test]
fn anything_else_keeps_the_node_with_the_fleet() {
    let before = wireless(AT_36, "");
    let other = wireless(
        "\toption channel '44'\n\toption htmode 'HE80'\n\toption country 'DE'\n",
        "",
    );
    assert_eq!(air(&before, &other, &radios()), None);
    let ssid = wireless(
        "\toption channel '44'\n\toption htmode 'HE80'\n",
        "\nconfig wifi-iface 'guest'\n\toption device 'radio1'\n\toption mode 'ap'\n",
    );
    assert_eq!(air(&before, &ssid, &radios()), None);
    assert_eq!(air(&before, &before, &radios()), None);
}

#[test]
fn a_mesh_radio_waits_for_the_fleets_cutover() {
    let mesh =
        "\nconfig wifi-iface 'mesh_radio1'\n\toption device 'radio1'\n\toption mode 'mesh'\n";
    let before = wireless(AT_36, mesh);
    let after = wireless("\toption channel '44'\n\toption htmode 'HE80'\n", mesh);
    assert_eq!(air(&before, &after, &radios()), None);
    let mut facts = radios();
    facts[1] = radio(
        "radio1",
        Band::G5,
        &[WifiIfaceMode::Ap, WifiIfaceMode::Mesh],
    );
    let plain = wireless("\toption channel '44'\n\toption htmode 'HE80'\n", "");
    assert_eq!(air(&wireless(AT_36, ""), &plain, &facts), None);
}

#[test]
fn a_radio_that_cannot_switch_reloads() {
    let before = wireless(AT_36, "");
    let auto = wireless("\toption channel 'auto'\n\toption htmode 'HE80'\n", "");
    let air_auto = air(&before, &auto, &radios()).unwrap();
    assert!(!air_auto.live());
    assert_eq!(air_auto.moves[0].switch, Err("its channel is automatic"));

    let wide = wireless("\toption channel '36'\n\toption htmode 'EHT320'\n", "");
    assert!(!air(&before, &wide, &radios()).unwrap().live());

    let mut facts = radios();
    facts[1] = radio("radio1", Band::G5, &[WifiIfaceMode::Ap, WifiIfaceMode::Sta]);
    let after = wireless("\toption channel '44'\n\toption htmode 'HE80'\n", "");
    let repeater = air(&before, &after, &facts).unwrap();
    assert_eq!(
        repeater.moves[0].switch,
        Err("it carries more than access points")
    );
    facts[1].up = false;
    assert_eq!(
        air(&before, &after, &facts).unwrap().moves[0].switch,
        Err("it is down")
    );
    assert_eq!(
        air(&before, &after, &facts[..1]).unwrap().moves[0].switch,
        Err("the node doesn't report it")
    );
}

#[test]
fn htmodes_name_standard_and_width() {
    use uniwrt_model::wifi::WifiStandard::*;
    assert_eq!(super::htmode("HE80"), Some((He, ChannelWidth::Mhz80)));
    assert_eq!(super::htmode("HT40+"), Some((Ht, ChannelWidth::Mhz40)));
    assert_eq!(super::htmode("VHT160"), Some((Vht, ChannelWidth::Mhz160)));
    assert_eq!(super::htmode("EHT320"), Some((Eht, ChannelWidth::Mhz320)));
    assert_eq!(super::htmode("NOHT"), Some((Legacy, ChannelWidth::Mhz20)));
    assert_eq!(super::htmode("HE"), None);
}
