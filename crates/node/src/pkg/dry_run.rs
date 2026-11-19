//! A change tried first ([`simulate`]: `apk … -s`, `opkg … --noaction`), and what the managers
//! say it would do ([`DryRun`]): what comes, what goes (with the dependencies only it needed),
//! and what a removal leaves because others need it (apk updates its world and keeps them;
//! opkg refuses).

/// What a dry run would do, by package name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DryRun {
    pub added: Vec<String>,
    pub upgraded: Vec<String>,
    pub removed: Vec<String>,
    /// Packages asked to go that others need, with those that need them.
    pub kept: Vec<(String, Vec<String>)>,
}

/// `argv` (an install, removal or upgrade of [`super`]) as a dry run: nothing changes.
pub fn simulate(mut argv: Vec<String>) -> Vec<String> {
    let flag = if argv.first().is_some_and(|tool| tool == "apk") {
        "-s"
    } else {
        "--noaction"
    };
    argv.insert(2.min(argv.len()), flag.to_owned());
    argv
}

/// Reads what apk or opkg printed for a dry run.
pub fn parse(output: &str) -> DryRun {
    let mut run = DryRun::default();
    let mut lines = output.lines().peekable();
    while let Some(line) = lines.next() {
        let line = unbullet(line);
        if let Some(rest) = apk_step(line) {
            let mut words = rest.split_whitespace();
            let (Some(verb), Some(name)) = (words.next(), words.next()) else {
                continue;
            };
            match verb {
                "Installing" => run.added.push(name.to_owned()),
                "Upgrading" | "Downgrading" | "Replacing" | "Reinstalling" => {
                    run.upgraded.push(name.to_owned());
                }
                "Purging" | "Removing" => run.removed.push(name.to_owned()),
                _ => {}
            }
        } else if line.starts_with("World updated, but the following packages are not removed") {
            while let Some(next) = lines.peek() {
                let Some((name, needed)) = next.trim().split_once(':') else {
                    break;
                };
                run.kept.push((
                    name.trim().to_owned(),
                    needed.split_whitespace().map(str::to_owned).collect(),
                ));
                lines.next();
            }
        } else if let Some(rest) = line.strip_prefix("Installing ")
            && rest.ends_with(" to root...")
        {
            push_first(&mut run.added, rest);
        } else if let Some(rest) = line.strip_prefix("Upgrading ")
            && rest.contains(" on root from ")
        {
            push_first(&mut run.upgraded, rest);
        } else if let Some(rest) = line.strip_prefix("Removing package ") {
            push_first(&mut run.removed, rest);
        } else if let Some(rest) = line.strip_prefix("print_dependents_warning: Package ")
            && let Some(name) = rest.strip_suffix(" is depended upon by packages:")
        {
            let mut needed = Vec::new();
            while let Some(next) = lines.peek() {
                let Some(dependent) = unbullet(next).strip_prefix("print_dependents_warning: \t")
                else {
                    break;
                };
                needed.push(dependent.trim().to_owned());
                lines.next();
            }
            run.kept.push((name.to_owned(), needed));
        }
    }
    run
}

/// opkg's error lines without their ` * `.
fn unbullet(line: &str) -> &str {
    let trimmed = line.trim_start();
    trimmed.strip_prefix("* ").unwrap_or(trimmed)
}

/// apk's `(1/2) Installing name (version)`, without the counter.
fn apk_step(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('(')?;
    let (counter, rest) = rest.split_once(") ")?;
    counter
        .split_once('/')
        .is_some_and(|(a, b)| {
            a.bytes().all(|c| c.is_ascii_digit()) && b.bytes().all(|c| c.is_ascii_digit())
        })
        .then_some(rest)
}

fn push_first(names: &mut Vec<String>, rest: &str) {
    if let Some(name) = rest.split_whitespace().next() {
        names.push(name.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn each_manager_gets_its_dry_run_flag() {
        assert_eq!(
            simulate(names(&["apk", "del", "tcpdump"])),
            ["apk", "del", "-s", "tcpdump"]
        );
        assert_eq!(
            simulate(names(&["opkg", "remove", "--autoremove", "tcpdump"])),
            ["opkg", "remove", "--noaction", "--autoremove", "tcpdump"]
        );
    }

    #[test]
    fn apk_s_dry_runs_read() {
        // As 25.12's apk 3.0.5 prints them.
        let add = "(1/2) Installing libpcap1 (1.10.6-r1)\n(2/2) Installing tcpdump (4.99.6-r1)\nOK: 12.4 MiB in 138 packages\n";
        assert_eq!(parse(add).added, ["libpcap1", "tcpdump"]);
        let up = "(1/2) Upgrading jshn (2026.06.19~7dd12784-r1 -> 2026.11.02~ff13b89e-r1)\n\
                  (2/2) Upgrading dnsmasq (2.93-r1 -> 2.93-r2)\nOK: 11.3 MiB in 136 packages\n";
        assert_eq!(parse(up).upgraded, ["jshn", "dnsmasq"]);
        let del = "(1/2) Purging tcpdump (4.99.6-r1)\n(2/2) Purging libpcap1 (1.10.6-r1)\nOK: 11.3 MiB in 136 packages\n";
        assert_eq!(parse(del).removed, ["tcpdump", "libpcap1"]);
        let kept = "World updated, but the following packages are not removed due to:\n  libpcap1: tcpdump\n\nOK: 12.4 MiB in 138 packages\n";
        let run = parse(kept);
        assert_eq!(run.removed, Vec::<String>::new());
        assert_eq!(run.kept, [("libpcap1".to_owned(), names(&["tcpdump"]))]);
    }

    #[test]
    fn opkg_s_dry_runs_read() {
        // As 24.10's opkg prints them.
        let add = "Installing tcpdump (4.99.5-r1) to root...\nDownloading https://downloads.openwrt.org/x/tcpdump.ipk\n\
                   Installing libpcap1 (1.10.5-r2) to root...\n";
        assert_eq!(parse(add).added, ["tcpdump", "libpcap1"]);
        let up = "Upgrading ucode-mod-ubus on root from 2025.07.18~3f64c808-r1 to 2025.07.18~3f64c808-r2...\n";
        assert_eq!(parse(up).upgraded, ["ucode-mod-ubus"]);
        let del = "Removing package tcpdump from root...\nlibpcap1 was autoinstalled and is now orphaned, removing.\n\
                   Removing package libpcap1 from root...\n";
        assert_eq!(parse(del).removed, ["tcpdump", "libpcap1"]);
        let refused = "No packages removed.\nCollected errors:\n\
                       * print_dependents_warning: Package libpcap1 is depended upon by packages:\n\
                       * print_dependents_warning: \ttcpdump\n\
                       * print_dependents_warning: These might cease to work if package libpcap1 is removed.\n";
        let run = parse(refused);
        assert_eq!(run.removed, Vec::<String>::new());
        assert_eq!(run.kept, [("libpcap1".to_owned(), names(&["tcpdump"]))]);
    }
}
