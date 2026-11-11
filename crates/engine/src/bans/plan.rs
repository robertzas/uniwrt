//! What to send a node's agent: pure, over what the controller wants kept off and what the node
//! is believed to hold.

use std::{collections::BTreeMap, time::Duration};

use uniwrt_core::{MacAddr, Timestamp};
use uniwrt_node::proto::ops::BanEntry;

/// The clients to keep off: blocked ones for good (`None`), kicked ones until a time.
pub(crate) type Wanted = BTreeMap<MacAddr, Option<Timestamp>>;
/// What a node holds: each client's ban, until when.
pub(crate) type Held = BTreeMap<MacAddr, Timestamp>;

/// How bans are sent.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Terms {
    /// The longest ban sent at once; longer ones are renewed.
    pub(crate) chunk: Duration,
    /// How long before a ban lapses it is renewed.
    pub(crate) renew_before: Duration,
}

/// The entries to send, or `None` when the node holds what is wanted: every wanted client
/// banned for long enough (renewing those within `renew_before` of lapsing), no other ban still
/// running there. A request carries every wanted ban, so a BSS that forgot them gets them all;
/// `force` sends it anyway. `kick` deauthenticates that client even without a ban.
pub(crate) fn request(
    wanted: &Wanted,
    held: &Held,
    now: Timestamp,
    terms: Terms,
    force: bool,
    kick: Option<MacAddr>,
) -> Option<Vec<BanEntry>> {
    let lapsing = |mac: &MacAddr, until: Option<Timestamp>| {
        held.get(mac).is_none_or(|&held| {
            held.saturating_duration_since(now) < terms.renew_before
                && until.is_none_or(|until| until > held)
        })
    };
    let lift = |mac: &MacAddr, held: &Timestamp| !wanted.contains_key(mac) && *held > now;
    let needed = force
        || kick.is_some()
        || wanted.iter().any(|(mac, until)| lapsing(mac, *until))
        || held.iter().any(|(mac, held)| lift(mac, held));
    if !needed {
        return None;
    }
    let mut entries: Vec<BanEntry> = wanted
        .iter()
        .filter_map(|(mac, until)| {
            let remaining = until.map_or(terms.chunk, |until| until.saturating_duration_since(now));
            let ms = u32::try_from(remaining.min(terms.chunk).as_millis()).unwrap_or(u32::MAX);
            (ms > 0).then(|| entry(*mac, ms, kick == Some(*mac)))
        })
        .collect();
    entries.extend(
        held.iter()
            .filter(|(mac, held)| lift(mac, held) && kick != Some(**mac))
            .map(|(mac, _)| entry(*mac, 0, false)),
    );
    if let Some(mac) = kick
        && !entries.iter().any(|e| e.kick)
    {
        entries.push(entry(mac, 0, true));
    }
    Some(entries)
}

/// What the node holds once it took `entries` sent at `at`.
pub(crate) fn took(held: &mut Held, entries: &[BanEntry], at: Timestamp, now: Timestamp) {
    for e in entries {
        let Ok(mac) = e.addr.parse::<MacAddr>() else {
            continue;
        };
        if e.ms == 0 {
            held.remove(&mac);
        } else {
            held.insert(mac, at.saturating_add(Duration::from_millis(e.ms.into())));
        }
    }
    held.retain(|_, until| *until > now);
}

fn entry(mac: MacAddr, ms: u32, kick: bool) -> BanEntry {
    BanEntry {
        addr: mac.to_string(),
        ms,
        kick,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Timestamp = Timestamp::from_unix_secs(1_000_000);
    const TERMS: Terms = Terms {
        chunk: Duration::from_mins(10),
        renew_before: Duration::from_mins(5),
    };

    fn mac(last: u8) -> MacAddr {
        MacAddr::new([2, 0, 0, 0, 0, last])
    }

    fn at(secs: u64) -> Timestamp {
        NOW.saturating_add(Duration::from_secs(secs))
    }

    fn sent(entries: &[BanEntry]) -> Vec<(String, u32, bool)> {
        entries
            .iter()
            .map(|e| (e.addr.clone(), e.ms, e.kick))
            .collect()
    }

    #[test]
    fn bans_are_sent_in_chunks_and_renewed_before_they_lapse() {
        let wanted = Wanted::from([(mac(1), None), (mac(2), Some(at(90)))]);
        let mut held = Held::new();
        let entries = request(&wanted, &held, NOW, TERMS, false, None).unwrap();
        assert_eq!(
            sent(&entries),
            [
                ("02:00:00:00:00:01".to_owned(), 600_000, false),
                ("02:00:00:00:00:02".to_owned(), 90_000, false),
            ]
        );
        took(&mut held, &entries, NOW, NOW);
        assert_eq!(held, Held::from([(mac(1), at(600)), (mac(2), at(90))]));
        // Held long enough: nothing to send; the kicked one's ban ends when it should.
        assert_eq!(request(&wanted, &held, at(60), TERMS, false, None), None);
        // Within five minutes of lapsing, every ban goes again.
        let entries = request(&wanted, &held, at(301), TERMS, false, None).unwrap();
        assert_eq!(entries.len(), 1, "the kick is over: {entries:?}");
        assert_eq!(entries[0].ms, 600_000);
        // Forced (a node back, or a wanted client seen there), it goes anyway.
        assert!(request(&wanted, &held, at(1), TERMS, true, None).is_some());
    }

    #[test]
    fn bans_no_longer_wanted_are_lifted_and_kicks_deauthenticate() {
        let held = Held::from([(mac(1), at(600)), (mac(3), at(10))]);
        let wanted = Wanted::from([(mac(1), None)]);
        let entries = request(&wanted, &held, at(20), TERMS, false, None);
        assert_eq!(entries, None, "mac 3's ban has lapsed");
        let entries = request(&Wanted::new(), &held, at(20), TERMS, false, None).unwrap();
        assert_eq!(sent(&entries), [("02:00:00:00:00:01".to_owned(), 0, false)]);
        let mut left = held.clone();
        took(&mut left, &entries, at(20), at(20));
        assert!(left.is_empty());

        // A kick without a ban; with one, its entry kicks.
        let entries = request(&wanted, &held, at(20), TERMS, false, Some(mac(4))).unwrap();
        assert_eq!(
            sent(&entries),
            [
                ("02:00:00:00:00:01".to_owned(), 600_000, false),
                ("02:00:00:00:00:04".to_owned(), 0, true),
            ]
        );
        let wanted = Wanted::from([(mac(4), Some(at(60)))]);
        let entries = request(&wanted, &Held::new(), NOW, TERMS, false, Some(mac(4))).unwrap();
        assert_eq!(
            sent(&entries),
            [("02:00:00:00:00:04".to_owned(), 60_000, true)]
        );
    }
}
