//! 802.11 information elements of a scan result (IEEE 802.11-2020, 9.4.2): the network's name,
//! channel and width, standards, security and load. Malformed elements are skipped.

use uniwrt_model::{
    rf::Security,
    wifi::{Band, ChannelWidth, WifiStandard},
};

const SSID: u8 = 0;
const DS_PARAMETER: u8 = 3;
const BSS_LOAD: u8 = 11;
const HT_CAPABILITIES: u8 = 45;
const RSN: u8 = 48;
const HT_OPERATION: u8 = 61;
const MESH_ID: u8 = 114;
const VHT_CAPABILITIES: u8 = 191;
const VHT_OPERATION: u8 = 192;
const VENDOR: u8 = 221;
const EXTENSION: u8 = 255;
// Element ID extensions
const HE_CAPABILITIES: u8 = 35;
const HE_OPERATION: u8 = 36;
const EHT_OPERATION: u8 = 106;
const EHT_CAPABILITIES: u8 = 108;

/// The privacy bit of the capability information field.
const PRIVACY: u16 = 0x0010;

/// What a BSS says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Elements {
    /// `None` when absent; empty or all NULs for a hidden network.
    pub ssid: Option<Vec<u8>>,
    pub mesh_id: Option<Vec<u8>>,
    /// The channel it is on (DS parameter set, else the HT operation's primary channel).
    pub channel: Option<u16>,
    ht: Option<HtOperation>,
    /// A width and the channel number at its center, from the newest operation element.
    vht: Option<(ChannelWidth, u16)>,
    he_6g: Option<(ChannelWidth, u16)>,
    eht: Option<(ChannelWidth, u16)>,
    pub standard: Option<WifiStandard>,
    rsn_akms: Option<Vec<u8>>,
    wpa: bool,
    pub stations: Option<u16>,
    pub utilization_percent: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HtOperation {
    primary: u8,
    /// +1 above, -1 below, 0 none.
    secondary: i8,
}

impl Elements {
    pub(super) fn parse(mut bytes: &[u8]) -> Self {
        let mut found = Self::default();
        while let [id, len, rest @ ..] = bytes {
            let len = usize::from(*len);
            let Some((body, next)) = rest.split_at_checked(len) else {
                break;
            };
            found.element(*id, body);
            bytes = next;
        }
        found
    }

    fn element(&mut self, id: u8, body: &[u8]) {
        match (id, body) {
            (SSID, _) => self.ssid = Some(body.to_vec()),
            (MESH_ID, _) => self.mesh_id = Some(body.to_vec()),
            (DS_PARAMETER, [channel, ..]) => self.channel = Some(u16::from(*channel)),
            (BSS_LOAD, [s0, s1, utilization, ..]) => {
                self.stations = Some(u16::from_le_bytes([*s0, *s1]));
                let percent = u16::from(*utilization) * 100 / 255;
                self.utilization_percent = u8::try_from(percent).ok();
            }
            (HT_CAPABILITIES, _) => self.newest(WifiStandard::Ht),
            (HT_OPERATION, [primary, info, ..]) => {
                self.newest(WifiStandard::Ht);
                // A secondary channel counts only where 40 MHz is allowed (STA channel width).
                let secondary = match (info & 0x03, info & 0x04 != 0) {
                    (1, true) => 1,
                    (3, true) => -1,
                    _ => 0,
                };
                self.ht = Some(HtOperation {
                    primary: *primary,
                    secondary,
                });
            }
            (VHT_CAPABILITIES, _) => self.newest(WifiStandard::Vht),
            (VHT_OPERATION, [width, ccfs0, ccfs1, ..]) => {
                self.newest(WifiStandard::Vht);
                self.vht = vht_width(*width, *ccfs0, *ccfs1);
            }
            (RSN, _) => self.rsn_akms = rsn_akms(body),
            (VENDOR, [0x00, 0x50, 0xf2, 0x01, ..]) => self.wpa = true,
            (EXTENSION, [ext, rest @ ..]) => match *ext {
                HE_CAPABILITIES => self.newest(WifiStandard::He),
                HE_OPERATION => {
                    self.newest(WifiStandard::He);
                    self.he_operation(rest);
                }
                EHT_CAPABILITIES => self.newest(WifiStandard::Eht),
                EHT_OPERATION => {
                    self.newest(WifiStandard::Eht);
                    self.eht = eht_width(rest);
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn newest(&mut self, standard: WifiStandard) {
        self.standard = self.standard.max(Some(standard));
    }

    /// HE operation: 3 bytes of parameters, the BSS color, the basic HE-MCS set (2), then a VHT
    /// operation (3) if bit 14, a co-hosted BSSID indicator (1) if bit 15 and the 6 GHz operation
    /// (5) if bit 17.
    fn he_operation(&mut self, body: &[u8]) {
        let [p0, p1, p2, _color, _mcs0, _mcs1, rest @ ..] = body else {
            return;
        };
        let params = u32::from_le_bytes([*p0, *p1, *p2, 0]);
        let mut rest = rest;
        if params & (1 << 14) != 0 {
            let Some((vht, next)) = rest.split_first_chunk::<3>() else {
                return;
            };
            self.vht = self.vht.or_else(|| vht_width(vht[0], vht[1], vht[2]));
            rest = next;
        }
        if params & (1 << 15) != 0 {
            rest = rest.get(1..).unwrap_or_default();
        }
        if params & (1 << 17) != 0
            && let [primary, control, ccfs0, ccfs1, ..] = rest
        {
            self.channel = self.channel.or(Some(u16::from(*primary)));
            self.he_6g = match control & 0x03 {
                0 => Some((ChannelWidth::Mhz20, u16::from(*ccfs0))),
                1 => Some((ChannelWidth::Mhz40, u16::from(*ccfs0))),
                2 => Some((ChannelWidth::Mhz80, u16::from(*ccfs0))),
                _ if *ccfs1 != 0 => Some((ChannelWidth::Mhz160, u16::from(*ccfs1))),
                _ => Some((ChannelWidth::Mhz160, u16::from(*ccfs0))),
            };
        }
    }

    /// The width and center frequency of the whole channel whose primary 20 MHz channel is at
    /// `frequency_mhz`.
    pub(super) fn width(&self, band: Band, frequency_mhz: u32) -> (ChannelWidth, u32) {
        let candidates = match band {
            Band::G6 => [self.eht, self.he_6g, None],
            Band::G5 => [self.eht, self.vht, None],
            Band::G2 | Band::G60 => [None, None, None],
        };
        if let Some((width, center)) = candidates.into_iter().flatten().next()
            && width > ChannelWidth::Mhz20
            && center != 0
        {
            return (width, band.frequency(center));
        }
        match self.ht {
            Some(HtOperation { secondary: 1, .. }) => (ChannelWidth::Mhz40, frequency_mhz + 10),
            Some(HtOperation { secondary: -1, .. }) => (ChannelWidth::Mhz40, frequency_mhz - 10),
            _ => (ChannelWidth::Mhz20, frequency_mhz),
        }
    }

    pub(super) fn security(&self, capability: Option<u16>) -> Security {
        if let Some(akms) = &self.rsn_akms {
            let has = |types: &[u8]| akms.iter().any(|akm| types.contains(akm));
            // 802.1X, FT over 802.1X, SHA-256, Suite B, FILS and FT-802.1X-SHA384
            if has(&[1, 3, 5, 11, 12, 13, 14, 15, 16, 17, 22, 23]) {
                return Security::Enterprise;
            }
            // SAE, FT-SAE and their group-dependent variants
            let sae = has(&[8, 9, 24, 25]);
            // PSK, FT-PSK, PSK-SHA256
            let psk = has(&[2, 4, 6]);
            return match (sae, psk) {
                (true, true) => Security::Wpa2Wpa3,
                (true, false) => Security::Wpa3,
                (false, false) if has(&[18]) => Security::Owe,
                // PSK, or an RSN element without an AKM we know: it still protects the network.
                (false, _) => Security::Wpa2,
            };
        }
        if self.wpa {
            Security::Wpa
        } else if capability.is_some_and(|c| c & PRIVACY != 0) {
            Security::Wep
        } else {
            Security::Open
        }
    }
}

/// A VHT operation's width and center channel: 80 MHz at CCFS0, 160 MHz at CCFS1 when the two
/// are 8 channels apart (or at CCFS0 in the deprecated encoding); 80+80 counts as its first 80.
fn vht_width(width: u8, ccfs0: u8, ccfs1: u8) -> Option<(ChannelWidth, u16)> {
    let (c0, c1) = (u16::from(ccfs0), u16::from(ccfs1));
    match width {
        1 if c1 != 0 && c0.abs_diff(c1) == 8 => Some((ChannelWidth::Mhz160, c1)),
        1 | 3 => Some((ChannelWidth::Mhz80, c0)),
        2 => Some((ChannelWidth::Mhz160, c0)),
        _ => None,
    }
}

/// EHT operation: a byte of parameters (bit 0: the operation information follows), the basic
/// EHT-MCS set (4), then the control (width in bits 0–2), CCFS0 and CCFS1.
fn eht_width(body: &[u8]) -> Option<(ChannelWidth, u16)> {
    let [params, _, _, _, _, control, ccfs0, ccfs1, ..] = body else {
        return None;
    };
    if params & 0x01 == 0 {
        return None;
    }
    let (c0, c1) = (u16::from(*ccfs0), u16::from(*ccfs1));
    match control & 0x07 {
        0 => Some((ChannelWidth::Mhz20, c0)),
        1 => Some((ChannelWidth::Mhz40, c0)),
        2 => Some((ChannelWidth::Mhz80, c0)),
        3 => Some((ChannelWidth::Mhz160, c1)),
        4 => Some((ChannelWidth::Mhz320, c1)),
        _ => None,
    }
}

/// The AKM suite types (00-0F-AC) of an RSN element: version (2), group cipher (4), pairwise
/// ciphers (count 2 + 4 each), AKMs (count 2 + 4 each).
fn rsn_akms(body: &[u8]) -> Option<Vec<u8>> {
    let rest = body.get(6..)?;
    let (count, rest) = rest.split_first_chunk::<2>()?;
    let rest = rest.get(4 * usize::from(u16::from_le_bytes(*count))..)?;
    let (count, rest) = rest.split_first_chunk::<2>()?;
    let count = usize::from(u16::from_le_bytes(*count));
    Some(
        rest.as_chunks::<4>()
            .0
            .iter()
            .take(count)
            .filter(|suite| suite[..3] == [0x00, 0x0f, 0xac])
            .map(|suite| suite[3])
            .collect(),
    )
}
