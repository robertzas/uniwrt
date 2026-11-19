//! The schedules (D-138): restart times, the LEDs schedule, and each node's own restart time.

use std::collections::BTreeSet;

use super::Issues;
use crate::site::{
    SiteConfig,
    schedules::{NodeReboot, RebootTime},
};

pub(super) fn schedules(config: &SiteConfig, issues: &mut Issues) {
    if let Some(time) = &config.schedules.reboot {
        reboot(time, "schedules/reboot", issues);
    }
    if let Some(schedule) = &config.schedules.leds_off {
        super::wireless::schedule(
            schedule,
            "schedules/leds_off",
            "A schedule without times never turns the LEDs off: add a time, or turn it off",
            issues,
        );
    }
    for node in &config.nodes {
        if let NodeReboot::Own(time) = &node.reboot {
            reboot(time, &format!("nodes/{}/reboot", node.node), issues);
        }
    }
}

fn reboot(time: &RebootTime, at: &str, issues: &mut Issues) {
    if time.days.is_empty() {
        issues.error(at, "A restart time is on one day at least");
    }
    let mut days = BTreeSet::new();
    if !time.days.iter().all(|day| days.insert(day)) {
        issues.error(at, "A day is listed twice");
    }
    if time.at >= 1440 {
        issues.error(at, "The time is within a day");
    }
}
