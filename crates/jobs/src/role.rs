//! A node's role, changed after adoption (D-100): the router that becomes a dumb access point,
//! or the access point promoted to the site's gateway. Only the stored role changes here; the
//! next apply renders what the role means, and the review shows it first.

use uniwrt_core::NodeId;
use uniwrt_model::node::{Node, NodeMode, NodeRole};
use uniwrt_store::NodeRepo as _;

use crate::{Error, Jobs, Result, apply, manage};

impl Jobs {
    /// Gives `node` the `role`, and returns the node as it is now. Refused while an apply or a
    /// job for the node runs, for a second gateway, and for a managed gateway: the site's
    /// routing runs on it, so another node must become the gateway first.
    pub async fn set_role(&self, node: NodeId, role: NodeRole) -> Result<Node> {
        let shared = &self.shared;
        if shared.busy(apply::TARGET) {
            return Err(Error::Busy {
                target: "the configuration".to_owned(),
            });
        }
        let target = manage::target(node);
        if shared.busy(&target) {
            return Err(Error::Busy { target });
        }
        let nodes = shared.store.nodes().await?;
        let current = nodes
            .iter()
            .find(|n| n.id == node)
            .ok_or(Error::NoNode(node))?;
        if current.role == role {
            return Ok(current.clone());
        }
        if role == NodeRole::Gateway
            && let Some(gateway) = nodes.iter().find(|n| n.role == NodeRole::Gateway)
        {
            return Err(Error::Refused(format!(
                "{} is the site's gateway already: make it an access point first",
                gateway.name
            )));
        }
        if current.role == NodeRole::Gateway && current.mode == NodeMode::Managed {
            return Err(Error::Refused(format!(
                "The site's routing runs on {}: turn its management off before it stops being the gateway",
                current.name
            )));
        }
        shared
            .store
            .set_node_role(node, role)
            .await?
            .ok_or(Error::NoNode(node))
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_core::{MacAddr, NodeId};
    use uniwrt_model::node::{NewNode, Node, NodeMode, NodeRole};
    use uniwrt_store::{NodeRepo as _, Store};

    use crate::{Error, testing};

    async fn node(store: &Store, octet: u8, role: NodeRole) -> Node {
        store
            .create_node(NewNode {
                name: format!("node{octet}"),
                mac: MacAddr::new([2, 0, 0, 0, 0, octet]),
                board_name: "x86,generic".to_owned(),
                host: format!("10.0.0.{octet}"),
                port: 22,
                role,
            })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_site_keeps_one_gateway_and_a_managed_one_stays() {
        let setup = testing::setup(false).await;
        let (jobs, store) = (&setup.jobs, &setup.store);
        let gateway = node(store, 1, NodeRole::Gateway).await;
        let ap = node(store, 2, NodeRole::Ap).await;

        let refused = jobs.set_role(ap.id, NodeRole::Gateway).await.unwrap_err();
        assert!(
            matches!(&refused, Error::Refused(why) if why.contains("node1")),
            "{refused}"
        );
        store
            .set_node_mode(gateway.id, NodeMode::Managed)
            .await
            .unwrap();
        let refused = jobs.set_role(gateway.id, NodeRole::Ap).await.unwrap_err();
        assert!(matches!(refused, Error::Refused(_)), "{refused}");

        store
            .set_node_mode(gateway.id, NodeMode::Observe)
            .await
            .unwrap();
        let demoted = jobs.set_role(gateway.id, NodeRole::Ap).await.unwrap();
        assert_eq!(demoted.role, NodeRole::Ap);
        let promoted = jobs.set_role(ap.id, NodeRole::Gateway).await.unwrap();
        assert_eq!(promoted.role, NodeRole::Gateway);
        let same = jobs.set_role(ap.id, NodeRole::Gateway).await.unwrap();
        assert_eq!(same, promoted);
        assert!(matches!(
            jobs.set_role(NodeId::new(99), NodeRole::Ap).await,
            Err(Error::NoNode(_))
        ));
    }
}
