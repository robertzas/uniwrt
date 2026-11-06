//! The WAN test (features/wan.md, Connectivity test; D-075): tries a WAN configuration on the
//! gateway and always rolls it back, so the wizard knows before it saves.
//!
//! Steps: connect (the gateway, managed, no transaction open) → plan (the latest revision with
//! this WAN, rendered; only the WAN's sections of the gateway's own `network` change, `patch`)
//! → stage → apply (the dead-man timer armed for an uplink change) → link → address → gateway
//! → DNS → HTTPS → IPv6 (each live, with a hint when it fails) → 6rd (only with a fallback,
//! when DHCPv6 delegated no prefix: a second trial with the preset's 6rd) → rollback (the
//! gateway holds what it had; its timer rolls back if the command is lost).
//!
//! It fails (`no_internet`, the failed step's hint as the message) when IPv4 doesn't reach the
//! internet: link, address, a default route, DNS or HTTPS. IPv6 failing fails only its step.
//! A WAN without IPv4 counts its IPv6 address and route as the address and gateway.

mod checks;
mod gateway;
mod patch;
mod run;

use uniwrt_core::UserId;
use uniwrt_model::{
    job::{JobKind, JobNode, JobNodeState, NewJob, StepName},
    node::Node,
    site::gateway::{WanConfig, WanIpv6},
};
use zeroize::Zeroizing;

pub(crate) use run::run;

/// A WAN to try.
#[derive(Debug, Clone)]
pub struct WanTest {
    pub wan: WanConfig,
    /// A PPPoE password not in the vault yet; without it, the one `wan` refers to.
    pub password: Option<Zeroizing<String>>,
    /// The IPv6 to try when DHCPv6 delegates no prefix: a preset's `auto` 6rd.
    pub fallback: Option<WanIpv6>,
}

pub(crate) fn job(gateway: &Node, test: &WanTest, by: Option<UserId>) -> NewJob {
    let mut steps = vec![
        StepName::Connect,
        StepName::Plan,
        StepName::Stage,
        StepName::Apply,
        StepName::Link,
        StepName::Address,
        StepName::Gateway,
        StepName::Dns,
        StepName::Https,
        StepName::Ipv6,
    ];
    if test.fallback.is_some() {
        steps.push(StepName::SixRd);
    }
    steps.push(StepName::Rollback);
    NewJob {
        kind: JobKind::WanTest,
        target: gateway.name.clone(),
        node: Some(gateway.id),
        revision: None,
        created_by: by,
        steps,
        nodes: vec![JobNode {
            node: gateway.id,
            name: gateway.name.clone(),
            state: JobNodeState::Pending,
            detail: None,
        }],
    }
}

#[cfg(test)]
mod tests;
