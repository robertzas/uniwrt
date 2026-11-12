//! The first-boot script of zero-touch images, for this controller (features/adoption.md,
//! Path B; D-102): its key, and where a device finds it: `uniwrt.<domain>` (the gateway's
//! record), then the address that record holds, else the one the admin reached it at.

use std::net::IpAddr;

use uniwrt_model::node::NodeRole;
use uniwrt_node::ztp::{Endpoint, first_boot};
use uniwrt_store::{DriftRepo as _, NodeRepo as _, RevisionRepo as _};

use crate::{Error, Jobs, Result};

/// The domain of a site without a revision yet (the site's default).
const DEFAULT_DOMAIN: &str = "lan";

impl Jobs {
    /// The script for an image of `role` that calls home on HTTPS `port`. `seen_at` is the
    /// address the admin reached the controller at, the fallback when the gateway holds no
    /// record of it.
    pub async fn ztp_script(
        &self,
        role: NodeRole,
        port: u16,
        seen_at: Option<IpAddr>,
    ) -> Result<String> {
        let store = &self.shared.store;
        let domain = store
            .head_revision()
            .await?
            .map_or_else(|| DEFAULT_DOMAIN.to_owned(), |head| head.config.site.domain);
        let mut endpoints = vec![Endpoint {
            host: format!(
                "{}.{}",
                uniwrt_model::site::gateway::CONTROLLER_HOST,
                domain.trim_end_matches('.')
            ),
            port,
        }];
        if let Some(ip) = self.held_controller().await?.or(seen_at) {
            endpoints.push(Endpoint {
                host: ip.to_string(),
                port,
            });
        }
        let key =
            self.shared.key.authorized_key().map_err(|err| {
                Error::Failed(format!("cannot encode the controller's key: {err}"))
            })?;
        first_boot(&key, &endpoints, role).map_err(|err| Error::Failed(err.to_string()))
    }

    /// The controller's address in the gateway's `uniwrt` record, as the last apply confirmed
    /// it.
    async fn held_controller(&self) -> Result<Option<IpAddr>> {
        let store = &self.shared.store;
        let Some(gateway) = store
            .nodes()
            .await?
            .into_iter()
            .find(|node| node.role == NodeRole::Gateway)
        else {
            return Ok(None);
        };
        let held = store
            .confirmed(Some(gateway.id))
            .await?
            .into_iter()
            .find(|confirmed| confirmed.package == "dhcp")
            .and_then(|dhcp| uniwrt_uci::parse("dhcp", &dhcp.text).ok())
            .and_then(|dhcp| {
                let packages = std::collections::BTreeMap::from([("dhcp".to_owned(), dhcp)]);
                uniwrt_render::held_controller(&packages)
            });
        Ok(held)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use uniwrt_core::JobId;
    use uniwrt_model::{
        apply::{Applied, AppliedNode},
        node::{NewNode, NodeRole},
        site::{
            SiteConfig,
            revision::{NewRevision, RevisionKind},
        },
    };
    use uniwrt_store::{ApplyRepo as _, NodeRepo as _, RevisionRepo as _};

    use crate::testing::setup;

    #[tokio::test]
    async fn the_script_calls_the_controllers_name_then_where_the_admin_found_it() {
        let setup = setup(true).await;
        let script = setup
            .jobs
            .ztp_script(NodeRole::Bridge, 8443, "192.168.1.20".parse().ok())
            .await
            .unwrap();
        assert!(script.contains("\nURLS='https://uniwrt.lan:8443 https://192.168.1.20:8443'\n"));
        assert!(script.contains("\nROLE='bridge'\n"));
        let key = setup.key.authorized_key().unwrap();
        assert!(script.contains(&format!("\nCONTROLLER_KEY='{}'\n", key.trim())));

        let named_only = setup
            .jobs
            .ztp_script(NodeRole::Ap, 443, None)
            .await
            .unwrap();
        assert!(named_only.contains("\nURLS='https://uniwrt.lan:443'\n"));
    }

    #[tokio::test]
    async fn the_gateways_record_beats_the_admins_address() {
        let setup = setup(true).await;
        let mut config = SiteConfig::defaults(None);
        config.site.domain = "home.arpa".to_owned();
        let revision = setup
            .store
            .commit_revision(
                NewRevision {
                    base: None,
                    kind: RevisionKind::Edit,
                    restored_from: None,
                    author_id: None,
                    author: None,
                    message: "domain".to_owned(),
                    config,
                },
                Vec::new(),
            )
            .await
            .unwrap();
        let gateway = setup
            .store
            .create_node(NewNode {
                name: "gateway".to_owned(),
                mac: "94:83:c4:00:00:01".parse().unwrap(),
                board_name: "glinet,gl-mt6000".to_owned(),
                host: "10.0.0.1".to_owned(),
                port: 22,
                role: NodeRole::Gateway,
            })
            .await
            .unwrap();
        let dhcp = "config domain\n\toption name 'uniwrt'\n\toption ip '10.0.0.5'\n";
        setup
            .store
            .record_applied(Applied {
                revision: revision.id,
                job: JobId::new(1),
                nodes: vec![AppliedNode {
                    node: gateway.id,
                    holds: true,
                    host: None,
                    packages: BTreeMap::from([("dhcp".to_owned(), dhcp.to_owned())]),
                }],
            })
            .await
            .unwrap();
        let script = setup
            .jobs
            .ztp_script(NodeRole::Ap, 443, "192.168.1.20".parse().ok())
            .await
            .unwrap();
        assert!(script.contains("\nURLS='https://uniwrt.home.arpa:443 https://10.0.0.5:443'\n"));
    }
}
