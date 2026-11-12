//! The wired backhaul's `mtu` op (features/mesh.md, Wired backhaul; D-096), as
//! `node-agent/mtu.uc` answers it: the device's MTU and the link-local addresses that answered
//! the probe's size, from the cables' ends as each node's `network` sets them
//! ([`World::probe`]).

use serde_json::{Value, json, value::RawValue};
use uniwrt_node::{
    Result,
    proto::ops::{Mtu, code},
};

use super::{SimLink, bad_request, field, remote};
use crate::world::World;

impl SimLink {
    pub(super) fn mtu(&self, world: &World, op: &str, args: &RawValue) -> Result<Value> {
        let dev: String = field(op, args, "dev")?;
        let size: u16 = field(op, args, "size")?;
        if !(Mtu::MIN..=Mtu::MAX).contains(&size) {
            return Err(bad_request(
                op,
                &format!("size must be between {} and {}", Mtu::MIN, Mtu::MAX),
            ));
        }
        let (mtu, peers) = world
            .probe(self.node, &dev, size)
            .ok_or_else(|| remote(op, code::NOT_FOUND, &format!("no device {dev}")))?;
        Ok(json!({"dev": dev, "size": size, "mtu": mtu, "peers": peers}))
    }
}
