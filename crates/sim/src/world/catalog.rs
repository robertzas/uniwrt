//! What the simulated fleet is made of: hardware models, firmware releases, the house's places
//! and the kinds of client, each with how it moves, what it supports and how much it talks.

/// A router model, as its board reports it.
pub(crate) struct Model {
    pub(crate) name: &'static str,
    pub(crate) board_name: &'static str,
    pub(crate) target: &'static str,
    pub(crate) arch: &'static str,
    pub(crate) cpu: &'static str,
    /// Bytes of RAM the kernel sees.
    pub(crate) memory: u64,
    /// KiB of the overlay filesystem, as `system info` counts it.
    pub(crate) overlay_kib: u64,
    /// Ports in `br-lan`.
    pub(crate) lan_ports: &'static [&'static str],
    /// The WAN port: the uplink of a gateway, the wired backhaul of an access point.
    pub(crate) wan_port: &'static str,
}

pub(crate) const GATEWAY_MODEL: Model = Model {
    name: "GL.iNet GL-MT6000",
    board_name: "glinet,gl-mt6000",
    target: "mediatek/filogic",
    arch: "aarch64_cortex-a53",
    cpu: "ARMv8 Processor rev 4",
    memory: 1_031_208_960,
    overlay_kib: 7_143_424,
    lan_ports: &["lan1", "lan2", "lan3", "lan4"],
    wan_port: "eth1",
};

pub(crate) const AP_MODEL: Model = Model {
    name: "GL.iNet GL-MT3000",
    board_name: "glinet,gl-mt3000",
    target: "mediatek/filogic",
    arch: "aarch64_cortex-a53",
    cpu: "ARMv8 Processor rev 4",
    memory: 512_098_304,
    overlay_kib: 216_064,
    lan_ports: &["eth1"],
    wan_port: "eth0",
};

/// The gateway's port that carries the wired backhaul (batman's `batwired`).
pub(crate) const GATEWAY_BACKHAUL_PORT: &str = "lan5";
/// The ISP's VLAN on the gateway's WAN port (the owner's ISP tags 201).
pub(crate) const WAN_VLAN: u16 = 201;

/// An OpenWrt release, as the captures from the hwsim lab report it.
pub(crate) struct Release {
    pub(crate) version: &'static str,
    pub(crate) revision: &'static str,
    pub(crate) kernel: &'static str,
    pub(crate) build_date: &'static str,
    pub(crate) package_manager: &'static str,
}

pub(crate) const RELEASES: [Release; 2] = [
    Release {
        version: "25.12.6",
        revision: "r33311-981c978eff",
        kernel: "6.12.58",
        build_date: "1792273538",
        package_manager: "apk",
    },
    Release {
        version: "24.10.8",
        revision: "r29233-443ec4032a",
        kernel: "6.6.110",
        build_date: "1784877710",
        package_manager: "opkg",
    },
];

/// Where the access points stand, in metres: the gateway first, then one per access point.
/// Further access points extend the house southwards.
pub(crate) const NODE_PLACES: [(&str, (f64, f64)); 4] = [
    ("gateway", (3.0, 7.0)),
    ("ap-living", (15.0, 4.0)),
    ("ap-office", (8.0, 15.0)),
    ("ap-garden", (24.0, 13.0)),
];

/// Where clients spend their time.
pub(crate) const SPOTS: [(f64, f64); 8] = [
    (14.0, 5.0),  // 0 living room
    (7.0, 3.0),   // 1 kitchen
    (10.0, 9.0),  // 2 hall
    (7.0, 16.0),  // 3 office
    (19.0, 16.0), // 4 bedroom
    (2.0, 14.0),  // 5 kids' room
    (28.0, 9.0),  // 6 garden
    (0.0, 1.0),   // 7 garage, where people come and go
];
pub(crate) const ENTRANCE: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Phone,
    Tablet,
    Laptop,
    Tv,
    Speaker,
    Iot,
    Camera,
    Pi,
    Desktop,
    Nas,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Standard {
    Ht,
    Vht,
    He,
}

/// How a client of a kind behaves.
#[expect(clippy::struct_excessive_bools, reason = "a kind's traits")]
pub(crate) struct KindSpec {
    pub(crate) kind: Kind,
    pub(crate) hostnames: &'static [&'static str],
    /// The vendor's OUI; `None` for a randomized, locally administered address.
    pub(crate) oui: Option<[u8; 3]>,
    /// Wired clients sit on a LAN port of the gateway, else of the nearest access point.
    pub(crate) wired: bool,
    /// Mean seconds between walks to another spot; `None` for clients that stay put.
    pub(crate) moves: Option<f64>,
    /// Whether it leaves the house now and then.
    pub(crate) leaves: bool,
    pub(crate) spots: &'static [usize],
    pub(crate) five_ghz: bool,
    pub(crate) standard: Standard,
    pub(crate) streams: u8,
    /// Fast transition (802.11r) and SAE (WPA3); the others authenticate with WPA2.
    pub(crate) ft: bool,
    pub(crate) sae: bool,
    /// Its transmit power relative to the access points', in dB.
    pub(crate) tx_offset: f64,
    /// Mean bytes per second it downloads while idle, and the factor while busy.
    pub(crate) rate: f64,
    pub(crate) burst: f64,
    pub(crate) ipv6: bool,
}

const fn spec(kind: Kind) -> KindSpec {
    KindSpec {
        kind,
        hostnames: &[],
        oui: None,
        wired: false,
        moves: None,
        leaves: false,
        spots: &[0],
        five_ghz: true,
        standard: Standard::He,
        streams: 2,
        ft: false,
        sae: false,
        tx_offset: -4.0,
        rate: 2_000.0,
        burst: 50.0,
        ipv6: false,
    }
}

pub(crate) static KINDS: [KindSpec; 10] = [
    KindSpec {
        hostnames: &[
            "iPhone",
            "Pixel-9",
            "Galaxy-S25",
            "iPhone-2",
            "OnePlus-13",
            "Pixel-8a",
        ],
        moves: Some(600.0),
        leaves: true,
        spots: &[0, 1, 2, 3, 4, 5, 6],
        ft: true,
        sae: true,
        tx_offset: -6.0,
        rate: 6_000.0,
        burst: 200.0,
        ipv6: true,
        ..spec(Kind::Phone)
    },
    KindSpec {
        hostnames: &["iPad", "Galaxy-Tab-S9"],
        moves: Some(1_800.0),
        spots: &[0, 4, 5],
        ft: true,
        sae: true,
        rate: 10_000.0,
        burst: 150.0,
        ipv6: true,
        ..spec(Kind::Tablet)
    },
    KindSpec {
        hostnames: &["MacBook-Air", "ThinkPad-X1", "XPS-13", "MacBook-Pro"],
        moves: Some(3_600.0),
        leaves: true,
        spots: &[3, 0, 1, 4],
        sae: true,
        tx_offset: -2.0,
        rate: 20_000.0,
        burst: 300.0,
        ipv6: true,
        ..spec(Kind::Laptop)
    },
    KindSpec {
        hostnames: &["Roku-Ultra", "living-room-tv"],
        oui: Some([0xb0, 0xa7, 0x37]),
        spots: &[0, 4],
        standard: Standard::Vht,
        tx_offset: 0.0,
        rate: 30_000.0,
        burst: 80.0,
        ..spec(Kind::Tv)
    },
    KindSpec {
        hostnames: &["Sonos-Kitchen", "Sonos-Living", "Sonos-Bedroom"],
        oui: Some([0x00, 0x0e, 0x58]),
        spots: &[1, 0, 4],
        standard: Standard::Vht,
        rate: 4_000.0,
        burst: 10.0,
        ..spec(Kind::Speaker)
    },
    KindSpec {
        hostnames: &[
            "thermostat",
            "plug-desk",
            "plug-tv",
            "doorbell",
            "air-sensor",
        ],
        oui: Some([0x24, 0x0a, 0xc4]),
        spots: &[2, 3, 0, 7, 4, 5],
        five_ghz: false,
        standard: Standard::Ht,
        streams: 1,
        tx_offset: -8.0,
        rate: 150.0,
        burst: 4.0,
        ..spec(Kind::Iot)
    },
    KindSpec {
        hostnames: &[],
        oui: Some([0x2c, 0xaa, 0x8e]),
        spots: &[6, 7],
        five_ghz: false,
        standard: Standard::Ht,
        streams: 1,
        tx_offset: -6.0,
        rate: 60_000.0,
        burst: 2.0,
        ..spec(Kind::Camera)
    },
    KindSpec {
        hostnames: &["raspberrypi", "homeassistant"],
        oui: Some([0xb8, 0x27, 0xeb]),
        spots: &[3, 2],
        standard: Standard::Vht,
        streams: 1,
        rate: 3_000.0,
        burst: 20.0,
        ipv6: true,
        ..spec(Kind::Pi)
    },
    KindSpec {
        hostnames: &["desktop"],
        oui: Some([0x00, 0x1b, 0x21]),
        wired: true,
        spots: &[3],
        rate: 40_000.0,
        burst: 100.0,
        ipv6: true,
        ..spec(Kind::Desktop)
    },
    KindSpec {
        hostnames: &["nas"],
        oui: Some([0x00, 0x11, 0x32]),
        wired: true,
        spots: &[2],
        rate: 25_000.0,
        burst: 200.0,
        ipv6: true,
        ..spec(Kind::Nas)
    },
];

/// The clients of a 30-client house, in an order where any prefix is a plausible smaller
/// house; larger houses repeat it.
pub(crate) const DECK: [Kind; 30] = {
    use Kind::{Camera, Desktop, Iot, Laptop, Nas, Phone, Pi, Speaker, Tablet, Tv};
    [
        Phone, Laptop, Tv, Iot, Phone, Speaker, Nas, Phone, Iot, Tablet, Laptop, Camera, Phone,
        Iot, Speaker, Pi, Phone, Laptop, Iot, Desktop, Tv, Phone, Iot, Tablet, Camera, Speaker,
        Laptop, Iot, Phone, Iot,
    ]
};

pub(crate) fn kind_spec(kind: Kind) -> &'static KindSpec {
    KINDS
        .iter()
        .find(|spec| spec.kind == kind)
        .unwrap_or(&KINDS[0])
}

/// GL.iNet's OUI, which the simulated routers' addresses start with.
pub(crate) const NODE_OUI: [u8; 3] = [0x94, 0x83, 0xc4];

/// Log lines a node writes now and then, by role: (gateway, priority, message).
pub(crate) const ROUTINE_LOGS: [(bool, u8, &str); 8] = [
    (true, 30, "odhcpd[1712]: A default route is present"),
    (
        true,
        29,
        "dropbear[3102]: Exit (root) from <192.168.1.120:52814>: Disconnected",
    ),
    (true, 30, "dnsmasq[2092]: read /etc/hosts - 12 names"),
    (
        true,
        30,
        "crond[1844]: USER root pid 4180 cmd /sbin/fstrim -a",
    ),
    (
        false,
        30,
        "netifd: lan (1611): udhcpc: sending renew to server 192.168.1.1",
    ),
    (
        false,
        30,
        "netifd: lan (1611): udhcpc: lease of 192.168.1.%ip obtained from 192.168.1.1, lease time 43200",
    ),
    (
        false,
        30,
        "crond[1503]: USER root pid 2214 cmd /usr/sbin/logrotate",
    ),
    (
        false,
        30,
        "dropbear[2790]: Pubkey auth succeeded for 'root' with ssh-ed25519 key",
    ),
];
