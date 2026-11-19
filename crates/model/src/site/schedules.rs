//! Controller-run schedules (features/network.md, Other services; D-138): restarts at a time of
//! the week, and the LEDs off at night. Both run on the controller over the **saved** site (no
//! apply: nothing in a node's configuration changes), on managed nodes only, in the site's time
//! zone; while the controller is down, nothing restarts and the LEDs stay as they are.

use serde::{Deserialize, Serialize};
use uniwrt_core::NodeId;

use super::{
    SiteConfig,
    wireless::{Schedule, Weekday},
};

/// How long after its time a restart may still start: a node offline or held by a job then
/// gets this long to come free.
pub const REBOOT_WINDOW_MIN: u32 = 60;

/// The site's schedules; both off for a new site.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Schedules {
    /// When the managed nodes that follow the site restart; none: never on a schedule.
    pub reboot: Option<RebootTime>,
    /// When the managed nodes' LEDs are off; none: always as the nodes have them.
    pub leds_off: Option<Schedule>,
}

/// A time of the week, on some days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RebootTime {
    /// The days it restarts on.
    pub days: Vec<Weekday>,
    /// The time, in minutes after midnight (0–1439).
    pub at: u16,
}

impl RebootTime {
    /// How many minutes ago it last came, at `minute` of `day` (local): 0 at the time itself,
    /// up to a week; `None` without days.
    pub fn since(&self, day: Weekday, minute: u16) -> Option<u32> {
        (0..=7u8).find_map(|back| {
            let then = Weekday::from_monday(day as u8 + 7 - back);
            let minutes = u32::from(back) * 1440 + u32::from(minute);
            (self.days.contains(&then) && minutes >= u32::from(self.at))
                .then(|| minutes - u32::from(self.at))
        })
    }

    /// Whether `minute` of `day` is within [`REBOOT_WINDOW_MIN`] of its time.
    pub fn due(&self, day: Weekday, minute: u16) -> bool {
        self.since(day, minute)
            .is_some_and(|since| since < REBOOT_WINDOW_MIN)
    }
}

/// Which schedule restarts a node.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeReboot {
    /// The site's.
    #[default]
    Site,
    /// None: it never restarts on a schedule.
    Never,
    /// Its own days and time.
    Own(RebootTime),
}

impl SiteConfig {
    /// When `node` restarts: its own time, the site's, or none. A node without an entry
    /// follows the site.
    pub fn reboot_time(&self, node: NodeId) -> Option<&RebootTime> {
        match self
            .nodes
            .iter()
            .find(|n| n.node == node)
            .map(|n| &n.reboot)
        {
            None | Some(NodeReboot::Site) => self.schedules.reboot.as_ref(),
            Some(NodeReboot::Never) => None,
            Some(NodeReboot::Own(time)) => Some(time),
        }
    }

    /// Whether the LEDs schedule has `node`'s LEDs off at `minute` of `day` (local). A node
    /// without an entry follows it.
    pub fn leds_off(&self, node: NodeId, day: Weekday, minute: u16) -> bool {
        let follows = self
            .nodes
            .iter()
            .find(|n| n.node == node)
            .is_none_or(|n| n.led_schedule);
        follows
            && self
                .schedules
                .leds_off
                .as_ref()
                .is_some_and(|schedule| schedule.is_on(day, minute))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time(days: &[Weekday], at: u16) -> RebootTime {
        RebootTime {
            days: days.to_vec(),
            at,
        }
    }

    #[test]
    fn a_time_comes_on_its_days_and_stays_due_for_the_window() {
        use Weekday::{Mon, Sat, Sun, Tue, Wed};
        let sunday = time(&[Sun], 4 * 60);
        assert_eq!(sunday.since(Sun, 240), Some(0));
        assert!(sunday.due(Sun, 240) && sunday.due(Sun, 299));
        assert!(!sunday.due(Sun, 300) && !sunday.due(Sun, 239));
        // Before its time on its day, it came a week ago.
        assert_eq!(sunday.since(Sun, 239), Some(7 * 1440 - 1));
        assert_eq!(sunday.since(Mon, 0), Some(20 * 60));
        assert_eq!(sunday.since(Sat, 1439), Some(6 * 1440 + 1199));

        // Late at night: the window runs into the next day.
        let nightly = time(&[Mon, Tue], 23 * 60 + 30);
        assert!(nightly.due(Tue, 29) && !nightly.due(Tue, 30));
        assert_eq!(nightly.since(Wed, 0), Some(30));
        assert_eq!(time(&[], 0).since(Mon, 0), None);
    }

    #[test]
    fn nodes_follow_the_site_unless_they_say_otherwise() {
        use super::super::{node::NodeConfig, wireless::Span};
        let (a, b, c) = (NodeId::new(1), NodeId::new(2), NodeId::new(3));
        let mut config = SiteConfig::defaults(None);
        assert_eq!(config.reboot_time(a), None);
        assert!(!config.leds_off(a, Weekday::Mon, 0));

        let site = time(&[Weekday::Sun], 240);
        let own = time(&[Weekday::Wed], 60);
        config.schedules = Schedules {
            reboot: Some(site.clone()),
            leds_off: Some(Schedule {
                spans: vec![Span {
                    days: Weekday::ALL.to_vec(),
                    from: 22 * 60,
                    to: 7 * 60,
                }],
            }),
        };
        let mut never = NodeConfig::new(b);
        never.reboot = NodeReboot::Never;
        never.led_schedule = false;
        let mut mine = NodeConfig::new(c);
        mine.reboot = NodeReboot::Own(own.clone());
        config.nodes = vec![never, mine];

        // A node without an entry follows the site.
        assert_eq!(config.reboot_time(a), Some(&site));
        assert_eq!(config.reboot_time(b), None);
        assert_eq!(config.reboot_time(c), Some(&own));
        assert!(config.leds_off(a, Weekday::Tue, 6 * 60));
        assert!(!config.leds_off(a, Weekday::Tue, 7 * 60));
        assert!(!config.leds_off(b, Weekday::Tue, 6 * 60));
        assert!(config.leds_off(c, Weekday::Tue, 23 * 60));
    }

    #[test]
    fn schedules_are_validated() {
        use std::collections::BTreeSet;

        use super::super::{node::NodeConfig, validate::validate};
        let node = NodeId::new(1);
        let mut config = SiteConfig::defaults(None);
        config.schedules = Schedules {
            reboot: Some(time(&[], 1440)),
            leds_off: Some(Schedule { spans: vec![] }),
        };
        let mut own = NodeConfig::new(node);
        own.reboot = NodeReboot::Own(time(&[Weekday::Mon, Weekday::Mon], 0));
        config.nodes = vec![own];
        let issues: Vec<_> = validate(&config, &BTreeSet::from([node]))
            .into_iter()
            .map(|issue| (issue.path, issue.message))
            .collect();
        let at = |path: &str| -> Vec<&str> {
            issues
                .iter()
                .filter(|(p, _)| p == path)
                .map(|(_, m)| m.as_str())
                .collect()
        };
        assert_eq!(
            at("schedules/reboot"),
            [
                "A restart time is on one day at least",
                "The time is within a day"
            ]
        );
        assert_eq!(at("schedules/leds_off").len(), 1);
        assert_eq!(at("nodes/1/reboot"), ["A day is listed twice"]);
    }
}
