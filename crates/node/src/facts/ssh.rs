//! dropbear's instances in the `services` set (D-150): each one's `running` and command line, as
//! `ubus call service list` reports them, read into the flags OpenWrt's init script sets from the
//! `dropbear` options: `-s` (no passwords), `-g` (no root password), `-l <device>`
//! (`DirectInterface`) and each `-p [address:]port`.

use serde::Deserialize;
use uniwrt_model::facts::SshServerFacts;

/// The most instances read: OpenWrt runs one per `dropbear` section.
const MAX_INSTANCES: usize = 16;
/// The most arguments read per command line.
const MAX_ARGS: usize = 64;

#[derive(Deserialize)]
pub(super) struct Instance {
    #[serde(default)]
    running: bool,
    #[serde(default)]
    command: Vec<String>,
}

pub(super) fn parse(raw: Vec<Instance>) -> Vec<SshServerFacts> {
    raw.into_iter().take(MAX_INSTANCES).map(instance).collect()
}

fn instance(raw: Instance) -> SshServerFacts {
    let mut facts = SshServerFacts {
        running: raw.running,
        password: true,
        root_password: true,
        device: None,
        listen: Vec::new(),
    };
    // The program comes first; the flags dropbear takes follow.
    let mut args = raw.command.into_iter().take(MAX_ARGS).skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-s" => facts.password = false,
            "-g" => facts.root_password = false,
            "-l" => facts.device = args.next(),
            "-p" => facts.listen.extend(args.next()),
            _ => {}
        }
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::{Instance, parse};

    fn instances(json: serde_json::Value) -> Vec<Instance> {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn the_flags_of_each_instance_are_read() {
        let facts = parse(instances(serde_json::json!([
            {"running": true, "command": ["/usr/sbin/dropbear", "-F", "-P",
                "/var/run/dropbear.main.pid", "-l", "br-lan.99", "-p", "22", "-s", "-g",
                "-K", "300", "-T", "3"]},
            {"running": false, "command": ["/usr/sbin/dropbear", "-F", "-P", "/var/run/x.pid",
                "-p", "10.0.0.1:2222", "-p", "fd00::1:2222"]},
            {"command": []}
        ])));
        assert_eq!(facts.len(), 3);
        let bound = &facts[0];
        assert!(bound.running);
        assert!(!bound.password && !bound.root_password);
        assert_eq!(bound.device.as_deref(), Some("br-lan.99"));
        assert_eq!(bound.listen, ["22"]);
        let open = &facts[1];
        assert!(!open.running);
        assert!(open.password && open.root_password);
        assert_eq!(open.device, None);
        assert_eq!(open.listen, ["10.0.0.1:2222", "fd00::1:2222"]);
        assert!(!facts[2].running && facts[2].password);
    }

    #[test]
    fn a_flag_without_its_value_reads_as_none() {
        let facts = parse(instances(serde_json::json!([
            {"running": true, "command": ["/usr/sbin/dropbear", "-p"]},
            {"running": true, "command": ["/usr/sbin/dropbear", "-l"]}
        ])));
        assert_eq!(facts[0].listen, Vec::<String>::new());
        assert_eq!(facts[1].device, None);
    }
}
