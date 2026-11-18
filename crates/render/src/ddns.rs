//! The `ddns` package (ddns-scripts; features/network.md, Other services; D-132): a `service`
//! section per DDNS entry, named by its id, which publishes the WAN's address (`wan`, or
//! `wan6` for an AAAA record) or the one a web service sees. Curated providers use
//! ddns-scripts' service definitions; a custom one its update URL. Updates go over HTTPS where
//! the provider has it, checked against the CA bundle; every 10 minutes the address is
//! compared, and sent anyway every 72 hours. ddns-scripts logs to syslog (notices and worse),
//! which reaches the controller's logs. With no entries on a gateway that holds the package,
//! it renders the global section alone, the service off.

use uniwrt_model::site::services::{Ddns, DdnsProvider, DdnsSource};
use uniwrt_uci::{Package, Section};

use crate::{Issue, Requirement, ext::SectionExt, plan::Plan};

/// ddns-scripts with its init script.
pub(crate) const REQUIRES: Requirement = Requirement {
    package: "ddns-scripts",
    service: Some("ddns"),
};

/// The service definitions of most providers.
const SERVICES: Requirement = Requirement {
    package: "ddns-scripts-services",
    service: None,
};

/// The CA bundle OpenWrt ships, which every fetch tool ddns-scripts uses reads.
const CA_BUNDLE: &str = "/etc/ssl/certs/ca-certificates.crt";

/// The web services that echo the address they see, for [`DdnsSource::Web`].
const IP_URL_V4: &str = "https://api.ipify.org";
const IP_URL_V6: &str = "https://api6.ipify.org";

/// The entries that run.
pub(crate) fn enabled<'a>(plan: &Plan<'a>) -> impl Iterator<Item = &'a Ddns> + use<'a> {
    plan.site.services.ddns.iter().filter(|d| d.enabled)
}

/// The feed packages the enabled entries need.
pub(crate) fn requires(plan: &Plan<'_>) -> Vec<Requirement> {
    let mut requires = vec![REQUIRES, SERVICES];
    for ddns in enabled(plan) {
        let extra = match ddns.provider {
            DdnsProvider::Cloudflare { .. } => Requirement {
                package: "ddns-scripts-cloudflare",
                service: None,
            },
            DdnsProvider::NoIp { .. } => Requirement {
                package: "ddns-scripts-noip",
                service: None,
            },
            _ => continue,
        };
        if !requires.contains(&extra) {
            requires.push(extra);
        }
    }
    requires
}

/// The package, or `None` where there is nothing to own: not the gateway, or no entries and no
/// package on the node.
pub(crate) fn render(plan: &mut Plan<'_>, held: bool) -> Option<Package> {
    plan.wan.as_ref()?;
    let entries = &plan.site.services.ddns;
    if entries.is_empty() && !held {
        return None;
    }
    let mut sections = vec![
        Section::named("ddns", "global")
            .with("ddns_dateformat", "%F %R")
            .with("ddns_loglines", "250")
            .with("upd_privateip", "0"),
    ];
    for ddns in entries {
        let secret = ddns.secret.as_ref().and_then(|secret| {
            let value = plan.secrets.get(secret).map(str::to_owned);
            if value.is_none() {
                plan.errors.push(Issue::new(
                    format!("services/ddns/{}", ddns.id),
                    format!(
                        "The vault holds no secret for the DDNS name {}",
                        ddns.hostname
                    ),
                ));
            }
            value
        });
        sections.push(service(ddns, secret));
    }
    let mut package = Package::new("ddns");
    package.sections = sections;
    Some(package)
}

/// One entry's `service` section.
fn service(ddns: &Ddns, secret: Option<String>) -> Section {
    let hostname = ddns.hostname.to_ascii_lowercase();
    let network = if ddns.ipv6 { crate::wan::WAN6 } else { "wan" };
    let (service, url, domain, username) = match &ddns.provider {
        DdnsProvider::Cloudflare { zone } => {
            let zone = zone.to_ascii_lowercase();
            let host = hostname
                .strip_suffix(&zone)
                .map_or("", |h| h.trim_end_matches('.'));
            (
                Some("cloudflare.com-v4"),
                None,
                format!("{host}@{zone}"),
                Some("Bearer".to_owned()),
            )
        }
        DdnsProvider::Duckdns => {
            let label = hostname.strip_suffix(".duckdns.org").unwrap_or(&hostname);
            (Some("duckdns.org"), None, label.to_owned(), None)
        }
        DdnsProvider::Desec => (
            Some("desec.io"),
            None,
            hostname.clone(),
            Some(hostname.clone()),
        ),
        DdnsProvider::NoIp { username } => (
            Some("no-ip.com"),
            None,
            hostname.clone(),
            Some(username.clone()),
        ),
        DdnsProvider::Dynu { username } => (
            Some("dynu.com"),
            None,
            hostname.clone(),
            Some(username.clone()),
        ),
        DdnsProvider::Freedns => (Some("afraid.org-v2-token"), None, hostname.clone(), None),
        DdnsProvider::Custom { url, username } => {
            (None, Some(url.as_str()), hostname.clone(), username.clone())
        }
    };
    // Custom URLs keep their scheme; the curated services all answer over HTTPS.
    let https = url.is_none_or(|url| url.starts_with("https://"));
    let section = Section::named("service", ddns.id.as_str())
        .with("enabled", if ddns.enabled { "1" } else { "0" })
        .maybe("service_name", service)
        .maybe("update_url", url)
        .with("lookup_host", hostname.as_str())
        .with("domain", domain)
        .maybe("username", username)
        .maybe("password", secret)
        .with("use_ipv6", if ddns.ipv6 { "1" } else { "0" })
        .with("interface", network);
    let section = match ddns.source {
        DdnsSource::Wan => section
            .with("ip_source", "network")
            .with("ip_network", network),
        DdnsSource::Web => section
            .with("ip_source", "web")
            .with("ip_url", if ddns.ipv6 { IP_URL_V6 } else { IP_URL_V4 }),
    };
    let section = if https {
        section.with("use_https", "1").with("cacert", CA_BUNDLE)
    } else {
        section.with("use_https", "0")
    };
    section
        .with("check_interval", "10")
        .with("check_unit", "minutes")
        .with("force_interval", "72")
        .with("force_unit", "hours")
        .with("use_syslog", "2")
}

#[cfg(test)]
mod tests {
    use uniwrt_model::site::{
        SecretRef,
        services::{Ddns, DdnsProvider, DdnsSource},
    };

    use super::service;

    fn entry(hostname: &str, provider: DdnsProvider) -> Ddns {
        Ddns {
            id: "home".to_owned(),
            enabled: true,
            hostname: hostname.to_owned(),
            provider,
            ipv6: false,
            source: DdnsSource::Wan,
            secret: Some(SecretRef::numbered(1)),
        }
    }

    fn options(ddns: &Ddns) -> Vec<(String, String)> {
        service(ddns, Some("tok".to_owned()))
            .options
            .iter()
            .filter_map(|e| Some((e.name.clone(), e.value.as_single()?.to_owned())))
            .filter(|(name, _)| {
                [
                    "service_name",
                    "update_url",
                    "domain",
                    "username",
                    "password",
                    "use_https",
                ]
                .contains(&name.as_str())
            })
            .collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect()
    }

    #[test]
    fn each_provider_fills_what_its_service_reads() {
        let cloudflare = |zone: &str| DdnsProvider::Cloudflare {
            zone: zone.to_owned(),
        };
        assert_eq!(
            options(&entry("home.Example.com", cloudflare("example.com"))),
            pairs(&[
                ("service_name", "cloudflare.com-v4"),
                ("domain", "home@example.com"),
                ("username", "Bearer"),
                ("password", "tok"),
                ("use_https", "1"),
            ])
        );
        assert_eq!(
            options(&entry("example.com", cloudflare("example.com")))[1],
            ("domain".to_owned(), "@example.com".to_owned())
        );
        assert_eq!(
            options(&entry("me.duckdns.org", DdnsProvider::Duckdns))[1],
            ("domain".to_owned(), "me".to_owned())
        );
        assert_eq!(
            options(&entry("me.dedyn.io", DdnsProvider::Desec))[2],
            ("username".to_owned(), "me.dedyn.io".to_owned())
        );
        let custom = DdnsProvider::Custom {
            url: "http://dyn.example/u?h=[DOMAIN]&ip=[IP]".to_owned(),
            username: None,
        };
        assert_eq!(
            options(&entry("home.example.com", custom)),
            pairs(&[
                ("update_url", "http://dyn.example/u?h=[DOMAIN]&ip=[IP]"),
                ("domain", "home.example.com"),
                ("password", "tok"),
                ("use_https", "0"),
            ])
        );
    }

    #[test]
    fn ipv6_publishes_wan6_and_the_web_source_asks_ipify() {
        let mut six = entry("me.dedyn.io", DdnsProvider::Desec);
        six.ipv6 = true;
        let section = service(&six, None);
        assert_eq!(section.option("ip_network"), Some("wan6"));
        assert_eq!(section.option("interface"), Some("wan6"));
        six.source = DdnsSource::Web;
        let section = service(&six, None);
        assert_eq!(section.option("ip_source"), Some("web"));
        assert_eq!(section.option("ip_url"), Some("https://api6.ipify.org"));
        assert_eq!(section.option("ip_network"), None);
        assert_eq!(section.option("password"), None);
    }
}
