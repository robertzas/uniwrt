//! Drift in the demo (features/lifecycle.md, Drift; D-074): a minute after a node's first
//! confirmed apply, someone changes one option of a package that apply wrote, as `LuCI` saves it
//! (the file parsed and written back as `uci commit` does), once per node. The option is one
//! that render writes and the import reads, so overwriting the drift reverts it and importing it
//! takes it into the site: the hostname, else the first radio's channel, whichever package the
//! apply wrote.

use std::collections::BTreeMap;

use uniwrt_uci::{Package, parse};

use crate::txn::Tool;

/// How long after the first confirmed apply the edit comes.
pub(crate) const AFTER_S: i64 = 60;

/// Whether the node's hand edit is still to come.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum HandEdit {
    #[default]
    Waiting,
    Done,
}

impl HandEdit {
    /// Edits `tool`'s files once the edit is due at `now_unix_s` and no transaction is open;
    /// the package edited.
    pub(crate) fn step(&mut self, tool: &mut Tool, now_unix_s: i64) -> Option<String> {
        if *self == Self::Done || tool.open().is_some() {
            return None;
        }
        let confirmed = tool.confirmed()?;
        if now_unix_s < confirmed.unix_s + AFTER_S {
            return None;
        }
        *self = Self::Done;
        let written = confirmed.packages.clone();
        edit(&mut tool.files, &written)
    }
}

/// Changes the first package of [`EDITS`] among `written`; its name.
fn edit(files: &mut BTreeMap<String, String>, written: &[String]) -> Option<String> {
    EDITS.iter().find_map(|(name, change)| {
        if !written.iter().any(|w| w == name) {
            return None;
        }
        let file = files.get_mut(*name)?;
        let mut package = parse(name, file).ok()?;
        change(&mut package)?;
        *file = package.to_file();
        Some((*name).to_owned())
    })
}

type Change = fn(&mut Package) -> Option<()>;

/// What the edit changes, by package, in the order it is chosen.
const EDITS: [(&str, Change); 2] = [("system", hostname), ("wireless", channel)];

/// The hostname, renamed with a `-1` (or back without it).
fn hostname(package: &mut Package) -> Option<()> {
    let section = package.sections.iter_mut().find(|s| s.kind == "system")?;
    let name = section.option("hostname")?;
    let next = match name.strip_suffix("-1") {
        Some(base) => base.to_owned(),
        None => format!("{name}-1"),
    };
    section.set("hostname", next);
    Some(())
}

/// The first radio's channel, pinned (or back to automatic).
fn channel(package: &mut Package) -> Option<()> {
    let section = package
        .sections
        .iter_mut()
        .find(|s| s.kind == "wifi-device")?;
    let next = match (section.option("channel"), section.option("band")) {
        (Some("auto") | None, Some("2g")) => "6",
        (Some("auto") | None, Some("6g")) => "37",
        (Some("auto") | None, _) => "36",
        _ => "auto",
    };
    section.set("channel", next);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYSTEM: &str = "\nconfig system\n\toption hostname 'ap'\n\toption log_size '128'\n";
    const WIRELESS: &str =
        "\nconfig wifi-device 'radio0'\n\toption band '5g'\n\toption channel 'auto'\n";

    fn files() -> BTreeMap<String, String> {
        [("system", SYSTEM), ("wireless", WIRELESS)]
            .into_iter()
            .map(|(n, t)| (n.to_owned(), t.to_owned()))
            .collect()
    }

    #[test]
    fn the_first_written_package_changes_in_one_option() {
        let mut files = files();
        let written = ["wireless".to_owned(), "system".to_owned()];
        assert_eq!(edit(&mut files, &written).as_deref(), Some("system"));
        let system = parse("system", &files["system"]).unwrap();
        assert_eq!(system.sections[0].option("hostname"), Some("ap-1"));
        assert_eq!(system.sections[0].option("log_size"), Some("128"));
        assert_eq!(files["wireless"], WIRELESS);
    }

    #[test]
    fn a_radio_on_auto_is_pinned_and_a_pinned_one_goes_back_to_auto() {
        let mut files = files();
        let written = ["wireless".to_owned()];
        assert_eq!(edit(&mut files, &written).as_deref(), Some("wireless"));
        let wireless = parse("wireless", &files["wireless"]).unwrap();
        assert_eq!(wireless.sections[0].option("channel"), Some("36"));
        edit(&mut files, &written);
        let wireless = parse("wireless", &files["wireless"]).unwrap();
        assert_eq!(wireless.sections[0].option("channel"), Some("auto"));
    }

    #[test]
    fn packages_the_apply_did_not_write_are_left_alone() {
        let mut files = files();
        assert_eq!(
            edit(&mut files, &["dhcp".to_owned(), "firewall".to_owned()]),
            None
        );
        assert_eq!(files, self::files());
    }
}
