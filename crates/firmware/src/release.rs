//! Release numbers and the upgrades on offer (pure; M7.1a1, D-121): the latest release of the
//! branch a node runs, and the next branch's latest. An upgrade never skips a branch, as
//! OpenWrt supports sysupgrade from the previous branch only.

use uniwrt_model::firmware::OfferKind;

use crate::catalog::Branch;

/// A release's number: `25.12.6`, or a candidate (`25.12.0-rc2`), which sorts before its final.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version {
    major: u32,
    minor: u32,
    patch: u32,
    /// The candidate's number, `u32::MAX` for a final release.
    rc: u32,
}

impl Version {
    pub(crate) fn parse(release: &str) -> Option<Self> {
        let (number, rc) = match release.split_once("-rc") {
            Some((number, rc)) => (number, rc.parse().ok()?),
            None => (release, u32::MAX),
        };
        let mut parts = number.split('.').map(|part| part.parse::<u32>().ok());
        let version = Self {
            major: parts.next()??,
            minor: parts.next()??,
            patch: parts.next()??,
            rc,
        };
        parts.next().is_none().then_some(version)
    }

    fn branch(self) -> (u32, u32) {
        (self.major, self.minor)
    }

    fn is_final(self) -> bool {
        self.rc == u32::MAX
    }
}

/// The branch a release belongs to, such as `25.12` for `25.12.0-rc2`.
fn branch_number(name: &str) -> Option<(u32, u32)> {
    let (major, minor) = name.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// The releases on offer for a node running `current`, newest first; or why there are none to
/// work out (a snapshot or an unknown number). Up to date gives an empty list.
pub(crate) fn offers(
    current: &str,
    branches: &[Branch],
) -> Result<Vec<(String, OfferKind)>, String> {
    let Some(running) = Version::parse(current) else {
        return Err(format!(
            "{current} isn't a release: snapshots get no upgrades on offer"
        ));
    };
    let mut offers = Vec::new();
    let next = branches
        .iter()
        .filter_map(|branch| Some((branch_number(&branch.name)?, branch)))
        .filter(|(number, _)| *number > running.branch())
        .min_by_key(|(number, _)| *number);
    if let Some((_, branch)) = next
        && let Some((_, release)) = latest(branch)
    {
        offers.push((release.clone(), OfferKind::Branch));
    }
    let own = branches
        .iter()
        .find(|branch| branch_number(&branch.name) == Some(running.branch()));
    if let Some((version, release)) = own.and_then(latest)
        && version > running
    {
        offers.push((release.clone(), OfferKind::Update));
    }
    Ok(offers)
}

/// A branch's latest final release.
fn latest(branch: &Branch) -> Option<(Version, &String)> {
    branch
        .versions
        .iter()
        .filter_map(|release| Some((Version::parse(release)?, release)))
        .filter(|(version, _)| version.is_final())
        .max_by_key(|(version, _)| *version)
}

/// The release notes of `release` on openwrt.org.
pub(crate) fn notes(release: &str) -> String {
    let branch = release.split('.').take(2).collect::<Vec<_>>().join(".");
    format!("https://openwrt.org/releases/{branch}/notes-{release}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(name: &str, versions: &[&str]) -> Branch {
        Branch {
            name: name.to_owned(),
            versions: versions.iter().map(|v| (*v).to_owned()).collect(),
            forked_at: None,
            changes: Vec::new(),
        }
    }

    fn catalog() -> Vec<Branch> {
        vec![
            branch(
                "25.12",
                &[
                    "25.12.6",
                    "25.12.5",
                    "25.12.0",
                    "25.12.0-rc5",
                    "25.12-SNAPSHOT",
                ],
            ),
            branch("24.10", &["24.10.8", "24.10.7", "24.10.0-rc1"]),
            branch("23.05", &["23.05.6"]),
        ]
    }

    fn names(offers: &[(String, OfferKind)]) -> Vec<(&str, OfferKind)> {
        offers.iter().map(|(r, k)| (r.as_str(), *k)).collect()
    }

    #[test]
    fn versions_order_candidates_before_their_final() {
        let v = |s| Version::parse(s).unwrap();
        assert!(v("25.12.0-rc5") < v("25.12.0"));
        assert!(v("25.12.0-rc2") < v("25.12.0-rc10"));
        assert!(v("24.10.8") < v("25.12.0-rc1"));
        assert!(v("25.12.10") > v("25.12.9"));
        assert_eq!(Version::parse("SNAPSHOT"), None);
        assert_eq!(Version::parse("25.12-SNAPSHOT"), None);
        assert_eq!(Version::parse("25.12.0-beta"), None);
    }

    #[test]
    fn an_old_release_gets_the_next_branch_and_its_own_latest() {
        let offers = offers("24.10.7", &catalog()).unwrap();
        assert_eq!(
            names(&offers),
            [
                ("25.12.6", OfferKind::Branch),
                ("24.10.8", OfferKind::Update)
            ]
        );
    }

    #[test]
    fn a_branch_is_never_skipped() {
        let offers = offers("23.05.6", &catalog()).unwrap();
        assert_eq!(names(&offers), [("24.10.8", OfferKind::Branch)]);
    }

    #[test]
    fn the_latest_release_is_up_to_date_and_a_candidate_gets_its_final() {
        assert_eq!(
            offers("25.12.6", &catalog()).unwrap(),
            [] as [(std::string::String, uniwrt_model::firmware::OfferKind); 0]
        );
        let offers = offers("25.12.0-rc5", &catalog()).unwrap();
        assert_eq!(names(&offers), [("25.12.6", OfferKind::Update)]);
    }

    #[test]
    fn a_next_branch_with_only_candidates_offers_nothing_yet() {
        let mut catalog = catalog();
        catalog.insert(0, branch("26.06", &["26.06.0-rc1"]));
        let offers = offers("25.12.6", &catalog).unwrap();
        assert_eq!(
            offers,
            [] as [(std::string::String, uniwrt_model::firmware::OfferKind); 0]
        );
    }

    #[test]
    fn snapshots_get_a_reason_instead() {
        assert!(offers("SNAPSHOT", &catalog()).is_err());
    }

    #[test]
    fn notes_name_the_branch() {
        assert_eq!(
            notes("25.12.6"),
            "https://openwrt.org/releases/25.12/notes-25.12.6"
        );
    }
}
