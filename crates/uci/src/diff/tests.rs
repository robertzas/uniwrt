use crate::{ApplyError, Package, Section, apply, diff, parse};

fn fixture(package: &str, text: &str) -> Package {
    parse(package, text).unwrap()
}

fn firewall() -> Package {
    fixture(
        "firewall",
        include_str!("../../fixtures/openwrt/gateway/firewall"),
    )
}

fn network() -> Package {
    fixture(
        "network",
        include_str!("../../fixtures/openwrt/gateway/network"),
    )
}

fn rule(package: &mut Package, index: usize) -> &mut Section {
    package
        .sections
        .iter_mut()
        .filter(|s| s.kind == "rule")
        .nth(index)
        .unwrap()
}

/// The diff's text, after checking that it applies.
fn shown(a: &Package, b: &Package) -> String {
    let d = diff(a, b);
    let applied = apply(a, &d).unwrap();
    assert!(diff(&applied, b).is_empty(), "{d}");
    d.to_string()
}

#[test]
fn an_edited_rule_is_changed() {
    let a = firewall();
    let mut b = a.clone();
    rule(&mut b, 1).set("family", "any");
    rule(&mut b, 1).remove("icmp_type");
    assert_eq!(
        shown(&a, &b),
        " firewall.@rule[1]=rule\n\
         -firewall.@rule[1].family='ipv4'\n\
         +firewall.@rule[1].family='any'\n\
         -firewall.@rule[1].icmp_type='echo-request'\n"
    );
}

#[test]
fn an_inserted_rule_is_added_alone() {
    let a = firewall();
    let mut b = a.clone();
    let at = b.sections.iter().position(|s| s.kind == "rule").unwrap();
    let ssh = Section::anonymous("rule")
        .with("name", "Allow-SSH")
        .with("src", "lan");
    b.sections.insert(at, ssh);
    assert_eq!(
        shown(&a, &b),
        "+firewall.@rule[0]=rule\n\
         +firewall.@rule[0].name='Allow-SSH'\n\
         +firewall.@rule[0].src='lan'\n"
    );
}

#[test]
fn a_rule_put_first_is_moved() {
    let a = firewall();
    let mut b = a.clone();
    let rules: Vec<usize> = (0..b.sections.len())
        .filter(|&i| b.sections[i].kind == "rule")
        .collect();
    let guest = b.sections.remove(rules[2]);
    b.sections.insert(rules[0], guest);
    assert_eq!(
        shown(&a, &b),
        " firewall.@rule[0]=rule (moved from @rule[2])\n"
    );
    // Moving it among other types only is no change.
    let mut c = a.clone();
    let include = c.sections.pop().unwrap();
    c.sections.insert(0, include);
    assert!(diff(&a, &c).is_empty());
}

#[test]
fn a_replaced_rule_is_removed_and_added() {
    let a = firewall();
    let mut b = a.clone();
    *rule(&mut b, 0) = Section::anonymous("rule")
        .with("name", "Block-Telnet")
        .with("dest_port", "23");
    assert_eq!(
        shown(&a, &b),
        "-firewall.@rule[0]=rule\n\
         -firewall.@rule[0].name='Allow-DHCP-Renew'\n\
         -firewall.@rule[0].src='wan'\n\
         -firewall.@rule[0].proto='udp'\n\
         -firewall.@rule[0].dest_port='68'\n\
         -firewall.@rule[0].target='ACCEPT'\n\
         -firewall.@rule[0].family='ipv4'\n\
         +firewall.@rule[0]=rule\n\
         +firewall.@rule[0].name='Block-Telnet'\n\
         +firewall.@rule[0].dest_port='23'\n"
    );
}

#[test]
fn named_sections_pair_by_name() {
    let a = network();
    let mut b = a.clone();
    let guest = b
        .sections
        .iter()
        .position(|s| s.name.as_deref() == Some("guest"))
        .unwrap();
    b.sections.remove(guest);
    "device".clone_into(&mut b.section_mut("mesh").unwrap().kind);
    let wan = b.section_mut("wan").unwrap();
    wan.set("dns", ["1.1.1.1", "8.8.8.8"]);
    wan.set("peerdns", "0");
    assert_eq!(
        shown(&a, &b),
        "-network.guest=interface\n\
         -network.guest.device='br-lan.20'\n\
         -network.guest.proto='static'\n\
         -network.guest.ipaddr='10.0.20.1'\n\
         -network.guest.netmask='255.255.255.0'\n \
         network.wan=interface\n\
         -network.wan.dns='1.1.1.1' '9.9.9.9'\n\
         +network.wan.dns='1.1.1.1' '8.8.8.8'\n\
         -network.mesh=interface\n\
         +network.mesh=device\n"
    );
}

#[test]
fn one_releases_defaults_against_the_others() {
    let a = fixture("dhcp", include_str!("../../fixtures/openwrt/24.10/dhcp"));
    let b = fixture("dhcp", include_str!("../../fixtures/openwrt/25.12/dhcp"));
    assert_eq!(
        shown(&a, &b),
        " dhcp.odhcpd=odhcpd\n\
         -dhcp.odhcpd.leasefile='/tmp/hosts/odhcpd'\n\
         +dhcp.odhcpd.leasefile='/tmp/odhcpd.leases'\n\
         +dhcp.odhcpd.piodir='/tmp/odhcpd-piodir'\n\
         +dhcp.odhcpd.hostsdir='/tmp/hosts'\n\
         -dhcp.odhcpd.piofolder='/tmp/odhcpd-piofolder'\n"
    );
}

#[test]
fn a_diff_applies_only_where_it_was_taken() {
    let a = firewall();
    let mut b = a.clone();
    rule(&mut b, 1).set("family", "any");
    let d = diff(&a, &b);
    let mut c = a.clone();
    rule(&mut c, 1).set("family", "ipv6");
    assert_eq!(apply(&c, &d), Err(ApplyError::Conflict("@rule[1]".into())));
    let empty = Package::new("firewall");
    assert_eq!(
        apply(&empty, &d),
        Err(ApplyError::Missing("@rule[1]".into()))
    );
}

#[test]
fn equal_rules_pair_in_order() {
    // Two equal rules, one removed: the later goes, not a change of both.
    let mut a = Package::new("firewall");
    let same = Section::anonymous("rule")
        .with("src", "wan")
        .with("target", "DROP");
    a.sections.extend([
        same.clone(),
        Section::anonymous("rule").with("src", "lan"),
        same,
    ]);
    let mut b = a.clone();
    b.sections.remove(2);
    assert_eq!(
        shown(&a, &b),
        "-firewall.@rule[2]=rule\n-firewall.@rule[2].src='wan'\n-firewall.@rule[2].target='DROP'\n"
    );
}

#[test]
fn thousands_of_hosts() {
    let host = |k: usize| {
        Section::anonymous("host")
            .with("name", format!("h{k}"))
            .with(
                "mac",
                format!("02:00:00:00:{:02x}:{:02x}", k / 256, k % 256),
            )
    };
    let mut a = Package::new("dhcp");
    a.sections.extend((0..5000).map(host));
    let mut b = a.clone();
    b.sections.insert(2500, host(9999));
    b.sections[1000].set("ip", "10.0.0.50");
    assert_eq!(
        shown(&a, &b),
        " dhcp.@host[1000]=host\n\
         +dhcp.@host[1000].ip='10.0.0.50'\n\
         +dhcp.@host[2500]=host\n\
         +dhcp.@host[2500].name='h9999'\n\
         +dhcp.@host[2500].mac='02:00:00:00:27:0f'\n"
    );
    // Reversed, past the size of the table: every host but one keeps its pair and moves.
    b.sections = a.sections.iter().rev().cloned().collect();
    let d = diff(&a, &b);
    assert_eq!(d.sections.len(), 4999);
    assert!(d.sections.iter().all(|s| s.moved && s.options.is_empty()));
    assert!(diff(&apply(&a, &d).unwrap(), &b).is_empty());
}
