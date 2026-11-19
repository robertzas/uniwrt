//! What the gateway's disk says of its dnsmasq query log (D-133): on while the main instance's
//! section of `dhcp` sets `logqueries` and dnsmasq runs; the names it blocks are a sample of
//! the blocklist the controller pushed to `/tmp` since the boot; the clients bypassing the
//! filter are those of the firewall's redirects to the unfiltered instance.

use std::collections::BTreeSet;

use uniwrt_model::site::blocking::{BLOCKLIST_FILE, UNFILTERED_PORT};

use super::commands::Disk;
use crate::world::QueryLog;

/// The most blocked names the world's clients ask for.
const SAMPLE: usize = 64;

/// What [`Disk::query_log`] last read it from.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct QueriesFrom {
    dhcp: String,
    firewall: String,
    /// The blocklist's first line (its header, naming its hash).
    list: Option<Vec<u8>>,
    running: bool,
}

impl Disk {
    /// Hands `changed` the gateway's query log when what sets it changed since the last call
    /// (`None` while dnsmasq logs no queries).
    pub(crate) fn query_log(&mut self, booted_ms: i64, changed: impl FnOnce(Option<QueryLog>)) {
        let text = |name: &str| self.tool.files.get(name).map_or("", String::as_str);
        let (dhcp, firewall) = (text("dhcp"), text("firewall"));
        let list = self.tmp.file(booted_ms, BLOCKLIST_FILE);
        let head = list.map(|list| list.split(|&b| b == b'\n').next().unwrap_or_default());
        let running = self.tool.services.get("dnsmasq") != Some(&false);
        if let Some(from) = &self.queries_from
            && from.dhcp == dhcp
            && from.firewall == firewall
            && from.list.as_deref() == head
            && from.running == running
        {
            return;
        }
        let log = read(dhcp, firewall, list).filter(|_| running);
        self.queries_from = Some(QueriesFrom {
            dhcp: dhcp.to_owned(),
            firewall: firewall.to_owned(),
            list: head.map(<[u8]>::to_vec),
            running,
        });
        changed(log);
    }
}

fn read(dhcp: &str, firewall: &str, list: Option<&[u8]>) -> Option<QueryLog> {
    let dhcp = uniwrt_uci::parse("dhcp", dhcp).ok()?;
    let main = dhcp
        .sections_of("dnsmasq")
        .find(|s| s.option("port").is_none_or(|port| port == "53"))?;
    if main.option("logqueries") != Some("1") {
        return None;
    }
    let domain = main.option("domain").unwrap_or("lan").to_owned();
    let unfiltered = UNFILTERED_PORT.to_string();
    let bypass: BTreeSet<_> = uniwrt_uci::parse("firewall", firewall).map_or_default(|firewall| {
        firewall
            .sections_of("redirect")
            .filter(|s| s.option("dest_port") == Some(unfiltered.as_str()))
            .flat_map(|s| s.get("src_mac").map_or_default(uniwrt_uci::Value::items))
            .filter_map(|mac| mac.parse().ok())
            .collect()
    });
    Some(QueryLog {
        blocked: list.map_or_default(sample),
        bypass,
        domain,
    })
}

/// Up to [`SAMPLE`] names of a compiled blocklist (`server=/<name>/` lines), spread over it.
fn sample(list: &[u8]) -> Vec<String> {
    let names: Vec<&str> = std::str::from_utf8(list)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.strip_prefix("server=/")?.strip_suffix('/'))
        .collect();
    let step = names.len().div_ceil(SAMPLE).max(1);
    names
        .iter()
        .step_by(step)
        .map(|&name| name.to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use uniwrt_core::MacAddr;

    use super::{QueryLog, read, sample};
    use crate::render::commands::Disk;

    const DHCP: &str = "config dnsmasq 'main'\n\toption domain 'home.arpa'\n\toption logqueries '1'\n\nconfig dnsmasq 'unfiltered'\n\toption port '5354'\n\toption logqueries '1'\n";
    const FIREWALL: &str = "config redirect\n\toption dest_port '5354'\n\tlist src_mac '02:00:00:00:04:01'\n\nconfig redirect\n\toption dest_port '8080'\n\tlist src_mac '02:00:00:00:04:02'\n";

    #[test]
    fn the_main_instance_sets_the_log_and_the_redirects_the_bypass() {
        let list = b"# uniwrt blocklist sha256:00\nserver=/ads.example/\nserver=/spy.example/\n";
        assert_eq!(
            read(DHCP, FIREWALL, Some(list)),
            Some(QueryLog {
                blocked: vec!["ads.example".to_owned(), "spy.example".to_owned()],
                bypass: [MacAddr::new([2, 0, 0, 0, 4, 1])].into(),
                domain: "home.arpa".to_owned(),
            })
        );
        let quiet = DHCP.replacen("\toption logqueries '1'\n", "", 1);
        assert_eq!(
            read(&quiet, FIREWALL, Some(list)),
            None,
            "the main instance's option"
        );
        let many = (0..1000).fold(String::new(), |mut list, i| {
            let _ = writeln!(list, "server=/n{i}.example/");
            list
        });
        let names = sample(many.as_bytes());
        assert_eq!((names.len(), names[0].as_str()), (63, "n0.example"));
    }

    #[test]
    fn the_log_is_handed_on_when_its_settings_change() {
        let mut disk = Disk::default();
        disk.tool.files.insert("dhcp".to_owned(), DHCP.to_owned());
        let mut seen = Vec::new();
        for _ in 0..2 {
            disk.query_log(0, |log| seen.push(log.map(|log| log.domain)));
        }
        disk.tool.services.insert("dnsmasq".to_owned(), false);
        disk.query_log(0, |log| seen.push(log.map(|log| log.domain)));
        assert_eq!(seen, [Some("home.arpa".to_owned()), None]);
    }
}
