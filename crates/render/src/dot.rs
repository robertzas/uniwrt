//! The `stubby` package (features/network.md, DHCP & local DNS; D-146): DNS over TLS on the
//! gateway, one daemon on `127.0.0.1` at [`PORT`] holding every DoT resolver of the site's
//! upstream, asked in order (`round_robin_upstreams '0'`), which the gateway's dnsmasq forwards
//! to where the first DoT resolver stands in the encrypted list. A query no resolver answers
//! within [`TIMEOUT_MS`] gets SERVFAIL, so dnsmasq moves on to the next layer at once. With DoT
//! off on a gateway that holds the package, it renders its settings without resolvers, the
//! service off.

use uniwrt_model::site::services::{DOT_PORT, DotResolver};
use uniwrt_uci::{Package, Section};

use crate::{Requirement, ext::SectionExt as _, plan::Plan};

/// The package DoT needs, with its init script.
pub(crate) const REQUIRES: Requirement = Requirement {
    package: "stubby",
    service: Some("stubby"),
};

/// The port stubby listens on (its own default).
pub(crate) const PORT: u16 = 5453;

/// How long stubby waits for an answer before it answers SERVFAIL: under the 2.5 s or more
/// clients wait before they ask again, so a dead resolver costs a query about this.
pub(crate) const TIMEOUT_MS: u32 = 2000;

/// The local server dnsmasq forwards to.
pub(crate) fn server() -> String {
    format!("127.0.0.1#{PORT}")
}

/// The site's DoT resolvers, in order.
pub(crate) fn resolvers<'a>(plan: &Plan<'a>) -> Vec<&'a DotResolver> {
    plan.site.services.upstream.dot().collect()
}

/// The package, or `None` where there is nothing to own: not the gateway, or DoT off and no
/// package on the node.
pub(crate) fn render(plan: &Plan<'_>, held: bool) -> Option<Package> {
    plan.wan.as_ref()?;
    let resolvers = resolvers(plan);
    if resolvers.is_empty() && !held {
        return None;
    }
    let mut sections = vec![
        Section::named("stubby", "global")
            .with("manual", "0")
            .with("trigger", "wan")
            .list("dns_transport", ["GETDNS_TRANSPORT_TLS"])
            .with("tls_authentication", "1")
            .with("tls_query_padding_blocksize", "128")
            .with("appdata_dir", "/var/lib/stubby")
            .with("edns_client_subnet_private", "1")
            .with("idle_timeout", "10000")
            .with("round_robin_upstreams", "0")
            .with("timeout", TIMEOUT_MS.to_string())
            .list("listen_address", [format!("127.0.0.1@{PORT}")]),
    ];
    for resolver in resolvers {
        for address in &resolver.addresses {
            sections.push(
                Section::anonymous("resolver")
                    .with("address", address.to_string())
                    .with("tls_auth_name", resolver.name.as_str())
                    .maybe(
                        "tls_port",
                        (resolver.port != DOT_PORT).then(|| resolver.port.to_string()),
                    )
                    .list(
                        "spki",
                        resolver.spki.iter().map(|pin| format!("sha256/{pin}")),
                    ),
            );
        }
    }
    let mut package = Package::new("stubby");
    package.sections = sections;
    Some(package)
}
