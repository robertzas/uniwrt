//! The built-in alert rules: `data/alert-rules.toml`, embedded.

use serde::Deserialize;
use uniwrt_model::{
    alerts::{RuleCondition, RuleSpec},
    events::EventLevel,
};

/// A built-in rule: its stable id, and what it says.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub id: String,
    pub spec: RuleSpec,
}

/// The embedded presets do not parse or check out (a test catches it before release).
#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("alert-rules.toml: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("alert-rules.toml: rule {id}: {reason}")]
    Invalid { id: String, reason: String },
    #[error("alert-rules.toml: rule {0} is there twice")]
    Duplicate(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    rule: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    name: String,
    level: EventLevel,
    #[serde(default = "enabled")]
    enabled: bool,
    cooldown_s: u32,
    when: RuleCondition,
}

const fn enabled() -> bool {
    true
}

/// The built-in rules, in the file's order.
pub fn presets() -> Result<Vec<Preset>, PresetError> {
    parse(include_str!("../../data/alert-rules.toml"))
}

fn parse(text: &str) -> Result<Vec<Preset>, PresetError> {
    let file: File = toml::from_str(text)?;
    let mut presets: Vec<Preset> = Vec::with_capacity(file.rule.len());
    for entry in file.rule {
        let spec = RuleSpec {
            name: entry.name,
            enabled: entry.enabled,
            level: entry.level,
            when: entry.when,
            cooldown_s: entry.cooldown_s,
        };
        spec.validate().map_err(|reason| PresetError::Invalid {
            id: entry.id.clone(),
            reason,
        })?;
        if presets.iter().any(|preset| preset.id == entry.id) {
            return Err(PresetError::Duplicate(entry.id));
        }
        presets.push(Preset { id: entry.id, spec });
    }
    Ok(presets)
}

#[cfg(test)]
mod tests {
    use uniwrt_model::{alerts::RuleCondition, events::EventKind};

    use super::{parse, presets};

    #[test]
    fn the_embedded_presets_check_out() {
        let presets = presets().expect("alert-rules.toml parses");
        assert!(presets.len() >= 15);
        let offline = presets.iter().find(|p| p.id == "node-offline").unwrap();
        let RuleCondition::Event(condition) = &offline.spec.when else {
            panic!("node-offline is an event rule");
        };
        assert_eq!(condition.kinds, ["node_offline"]);
        assert!(presets.iter().any(|p| !p.spec.enabled));
        // Every kind named is a real event's.
        let known = [
            EventKind::NodeOffline {
                reasons: Vec::new(),
            }
            .name(),
            "node_degraded",
            "node_rebooted",
            "flash_low",
            "job_failed",
            "config_drift",
            "wan_down",
            "mesh_link_down",
            "mesh_link_weak",
            "log_pattern",
            "new_client",
        ];
        for preset in &presets {
            if let RuleCondition::Event(condition) = &preset.spec.when {
                for kind in &condition.kinds {
                    assert!(known.contains(&kind.as_str()), "{}: {kind}", preset.id);
                }
            }
        }
    }

    #[test]
    fn refuses_bad_files() {
        let rule = |id: &str, threshold: f64| {
            format!(
                "[[rule]]\nid = \"{id}\"\nname = \"Hot\"\nlevel = \"warning\"\ncooldown_s = 0\n\
                 when = {{ type = \"metric\", metric = \"cpu\", entities = \"node:*\", \
                 op = \"above\", threshold = {threshold:?}, clear = 80.0, for_s = 60 }}\n"
            )
        };
        assert!(parse(&rule("a", 90.0)).is_ok());
        assert!(
            parse(&rule("a", 70.0)).is_err(),
            "clears above the threshold"
        );
        assert!(parse(&format!("{}{}", rule("a", 90.0), rule("a", 90.0))).is_err());
        assert!(parse("[[rule]]\nid = \"x\"\nsurprise = 1\n").is_err());
    }
}
