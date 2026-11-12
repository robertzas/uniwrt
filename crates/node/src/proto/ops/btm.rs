//! `btm`: a BSS transition request to one client, mirrored by `node-agent/btm.uc`
//! (features/wifi.md, Steering & balancing: controller-sent transition requests; D-093).

use serde::{Deserialize, Serialize};

use super::Op;

/// Asks a client to move: an 802.11v BSS transition request from the access point it is
/// connected to, with the MBO fields that 24.10's and 25.12's hostapd need (usteer 2022.08's
/// requests lack them, D-091). The client's answer arrives as a `bss-transition-response` push
/// on the `hostapd` topic. Fails `not_found` where the client isn't connected, `unsupported`
/// where hostapd has no 802.11v (the basic wpad).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Btm {
    /// In lower case, as hostapd writes it.
    pub addr: String,
    /// The candidates, best first: neighbor report elements in lower-case hex, each with its
    /// preference ([`candidate`]). With candidates the request is abridged.
    pub neighbors: Vec<String>,
    pub reason: MboReason,
    /// Tells the client it will be disconnected; then hostapd does so after `kick_ms` (0: it
    /// doesn't), and an MBO client stays away from this access point for `reassoc_s`.
    pub imminent: bool,
    pub kick_ms: u32,
    pub reassoc_s: u8,
}

impl Btm {
    pub const MAX_NEIGHBORS: usize = 16;
    pub const MAX_KICK_MS: u32 = 60_000;
}

impl Op for Btm {
    const NAME: &'static str = "btm";
    type Output = BtmSent;
}

/// MBO's transition reasons that steering gives (MBO 4.2.6, Transition Reason Code).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(into = "u8")]
pub enum MboReason {
    #[default]
    Unspecified,
    /// The access point sheds clients of a busy channel.
    LoadBalancing,
    /// The client's signal is low here.
    LowSignal,
    /// A better access point serves it, such as the same node's 5 GHz one.
    PremiumAp,
}

impl From<MboReason> for u8 {
    fn from(reason: MboReason) -> u8 {
        match reason {
            MboReason::Unspecified => 0,
            MboReason::LoadBalancing => 4,
            MboReason::LowSignal => 5,
            MboReason::PremiumAp => 9,
        }
    }
}

/// The request hostapd sent: from which access point, its dialog token (the answer echoes it),
/// the disconnect timer in beacon intervals (0: none), and whether the client does MBO.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BtmSent {
    pub ifname: String,
    pub token: u8,
    pub timer: u16,
    pub mbo: bool,
}

/// A candidate for [`Btm::neighbors`]: `report` (a BSS's own neighbor report element, as
/// `rrm_nr_get_own` gives it, in hex) with a BSS Transition Candidate Preference subelement
/// (id 3; 1 to 255, higher preferred) put among its subelements in order of id. `None` if
/// `report` isn't a report element's body in hex.
pub fn candidate(report: &str, preference: u8) -> Option<String> {
    // BSSID (6), BSSID information (4), operating class, channel and PHY type: 13 bytes, then
    // subelements (id, length, body).
    const FIXED: usize = 13 * 2;
    let report = report.to_ascii_lowercase();
    if report.len() < FIXED
        || !report.len().is_multiple_of(2)
        || !report.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(report.get(at..at + 2)?, 16).ok();
    let mut at = FIXED;
    // Before the first subelement whose id is above 3; an existing preference is replaced.
    let mut insert = report.len();
    let mut skip = None;
    while at < report.len() {
        let id = byte(at)?;
        let len = usize::from(byte(at + 2)?);
        let end = at + 4 + len * 2;
        if end > report.len() {
            return None;
        }
        if id == 3 {
            skip = Some(at..end);
        }
        if id > 3 {
            insert = at;
            break;
        }
        at = end;
    }
    let old = skip.unwrap_or(insert..insert);
    Some(format!(
        "{}0301{preference:02x}{}",
        &report[..old.start],
        &report[old.end..]
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A report's fixed part: BSSID, BSSID information, operating class 115, channel 36, VHT.
    const FIXED: &str = "020000000100ef090000732409";

    #[test]
    fn requests_go_as_the_agent_reads_them() {
        let btm = Btm {
            addr: "02:00:00:00:04:00".into(),
            neighbors: vec!["aa".repeat(13)],
            reason: MboReason::LowSignal,
            imminent: true,
            kick_ms: 10_000,
            reassoc_s: 30,
        };
        assert_eq!(
            serde_json::to_value(btm).unwrap(),
            json!({"addr": "02:00:00:00:04:00", "neighbors": ["aa".repeat(13)], "reason": 5,
                "imminent": true, "kick_ms": 10000, "reassoc_s": 30})
        );
        let sent: BtmSent = serde_json::from_value(
            json!({"ifname": "phy0-ap0", "token": 7, "timer": 98, "mbo": false}),
        )
        .unwrap();
        assert_eq!(sent.timer, 98);
    }

    #[test]
    fn a_candidate_takes_its_preference_among_the_subelements() {
        let fixed = FIXED;
        // No subelements: appended.
        assert_eq!(candidate(fixed, 255), Some(format!("{fixed}0301ff")));
        // Upper case is lowered; a wide bandwidth channel subelement (6) stays after it.
        let wide = format!("{}0603012a00", fixed.to_ascii_uppercase());
        assert_eq!(
            candidate(&wide, 200),
            Some(format!("{fixed}0301c80603012a00"))
        );
        // A TSF subelement (1) stays before it, and a preference already there is replaced.
        let tsf = format!("{fixed}01040000000003010a");
        assert_eq!(
            candidate(&tsf, 1),
            Some(format!("{fixed}010400000000030101"))
        );
        // Not a report: too short, odd, not hex, or a subelement running past the end.
        assert_eq!(candidate("aabb", 1), None);
        assert_eq!(candidate(&format!("{fixed}0"), 1), None);
        assert_eq!(candidate(&format!("{fixed}zz"), 1), None);
        assert_eq!(candidate(&format!("{fixed}0605"), 1), None);
    }
}
