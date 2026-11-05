//! The site's settings, from the gateway: its time zone and NTP servers (`system`) and the
//! local domain (dnsmasq's `domain`).

use uniwrt_model::site::{SiteConfig, tz};

use crate::{Source, read};

pub(crate) fn import(site: &mut SiteConfig, gateway: &Source<'_>) {
    let packages = gateway.packages;
    if let Some(system) = read::first(packages, "system", "system") {
        let named = system.option("zonename").filter(|z| tz::posix(z).is_some());
        let utc = matches!(system.option("timezone"), Some("UTC0" | "GMT0")).then_some("UTC");
        if let Some(zone) = named.or(utc) {
            zone.clone_into(&mut site.site.timezone);
        }
    }
    let servers: Vec<String> = packages
        .get("system")
        .and_then(|p| p.section("ntp"))
        .map(|ntp| {
            read::items(ntp, "server")
                .into_iter()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if !servers.is_empty() {
        site.site.ntp_servers = servers;
    }
    if let Some(domain) = read::first(packages, "dhcp", "dnsmasq").and_then(|d| d.option("domain"))
        && !domain.is_empty()
    {
        domain.clone_into(&mut site.site.domain);
    }
}
