//! The nodes' second: reboots (an access point goes down and boots again, its clients move
//! to the others), drifting load, memory and temperature, the radios' noise and air time, and
//! the routine lines of their logs.

use super::{
    Counters, Happening, Logd, What, World, catalog::ROUTINE_LOGS, clients::LEASE_MS, drift,
    radio::Band,
};

/// How long a reboot keeps an access point away, and how long it has been booting by then.
const REBOOT_S: u64 = 80;
const BOOT_MS: i64 = 70_000;

impl World {
    pub(super) fn step_nodes(&mut self, out: &mut Vec<Happening>) {
        let now = self.now_s;
        for node in 0..self.nodes.len() {
            if self.nodes[node]
                .down_until
                .is_some_and(|until| now >= until)
            {
                self.boot(node, out);
            }
        }
        if let Some(mean) = self.reboot_mean
            && now >= self.next_reboot
        {
            self.next_reboot = now + self.rng.exp(mean).max(300.0) as u64;
            let up: Vec<_> = (1..self.nodes.len())
                .filter(|&node| self.nodes[node].up())
                .collect();
            if !up.is_empty() {
                let node = up[self.rng.below(up.len() as u64) as usize];
                self.reboot(node, out);
            }
        }
        for node in 0..self.nodes.len() {
            if !self.nodes[node].up() {
                continue;
            }
            let clients = self.stations(node).count() as f64;
            let rng = &mut self.rng;
            let at = &mut self.nodes[node];
            let base = if at.gateway { 0.25 } else { 0.08 };
            at.load = drift(rng, at.load, base + clients * 0.01, 0.05, 0.02).max(0.0);
            at.cpu = drift(rng, at.cpu, 2.0 + at.load * 12.0, 0.1, 0.8).clamp(0.5, 100.0);
            let memory = if at.gateway { 0.24 } else { 0.36 };
            at.memory_used = drift(rng, at.memory_used, memory, 0.01, 0.002).clamp(0.1, 0.9);
            let temp = if at.gateway { 54.0 } else { 47.0 } + at.cpu * 0.2;
            at.temp = drift(rng, at.temp, temp, 0.02, 0.15);
            if self.lease_due(node) {
                self.node_lease(node, out);
            }
            if self.rng.chance(1.0 / 400.0) {
                self.routine_log(node);
            }
        }
    }

    /// Air time and noise follow the clients and their traffic.
    pub(super) fn step_air(&mut self) {
        for node in 0..self.nodes.len() {
            for band in Band::BOTH {
                let (clients, bytes) = self
                    .stations(node)
                    .filter(|(_, assoc)| assoc.band == band)
                    .fold((0.0, 0_u64), |(n, bytes), (_, assoc)| {
                        (n + 1.0, bytes + assoc.counters.tx_bytes)
                    });
                let rng = &mut self.rng;
                let radio = &mut self.nodes[node].radios[band.index()];
                let floor = if band == Band::G2 { -90.0 } else { -95.0 };
                radio.noise = drift(rng, radio.noise, floor, 0.05, 0.4);
                // A busier band: 2.4 GHz hears the neighbours' networks too.
                let neighbours = if band == Band::G2 { 18.0 } else { 4.0 };
                let mean = neighbours + clients * 1.5 + (bytes as f64).log10().max(0.0) * 0.5;
                radio.busy = drift(rng, radio.busy, mean, 0.1, 1.0).clamp(1.0, 95.0);
            }
        }
    }

    /// Takes access point `node` down for a reboot; `false` for the gateway or a node that is
    /// already down.
    pub(crate) fn reboot(&mut self, node: usize, out: &mut Vec<Happening>) -> bool {
        if node == 0 || node >= self.nodes.len() || !self.nodes[node].up() {
            return false;
        }
        self.nodes[node].down_until = Some(self.now_s + REBOOT_S);
        for client in &mut self.clients {
            if client
                .assoc
                .as_ref()
                .is_some_and(|assoc| assoc.node == node)
            {
                client.assoc = None;
                client.hold_until = self.now_s + 3 + self.rng.below(8);
            }
            if client.farewell.is_some_and(|(_, from, _)| from == node) {
                client.farewell = None;
            }
        }
        out.push(Happening {
            node,
            at: self.now(),
            what: What::Down,
        });
        true
    }

    fn boot(&mut self, node: usize, out: &mut Vec<Happening>) {
        let now = self.now_ms();
        let booted = now - BOOT_MS;
        let ip = self.nodes[node].ip;
        let at = &mut self.nodes[node];
        at.down_until = None;
        at.booted_ms = booted;
        at.load = 1.2;
        at.cpu = 30.0;
        for counters in at.ports.values_mut() {
            *counters = Counters::default();
        }
        for radio in &mut at.radios {
            radio.ap = Counters::default();
            radio.mesh = Counters::default();
        }
        at.logd = Logd::starting_at(1);
        let kernel = at.release.kernel;
        let wan = at.model.wan_port;
        let mut lines = vec![
            (
                6,
                "kernel: Booting Linux on physical CPU 0x0000000000 [0x410fd034]".to_owned(),
            ),
            (
                5,
                format!("kernel: Linux version {kernel} (builder@buildhost) #0 SMP"),
            ),
            (30, "procd: - init -".to_owned()),
            (
                6,
                "kernel: batman_adv: bat0: Adding interface: phy1-mesh0".to_owned(),
            ),
        ];
        if at.wired {
            lines.push((
                6,
                format!("kernel: batman_adv: bat0: Adding interface: {wan}"),
            ));
        }
        lines.extend([
            (29, "netifd: Interface 'lan' is now up".to_owned()),
            (29, "hostapd: phy0-ap0: interface state COUNTRY_UPDATE->ENABLED".to_owned()),
            (29, "hostapd: phy1-ap0: interface state COUNTRY_UPDATE->ENABLED".to_owned()),
            (
                30,
                format!(
                    "netifd: lan (1611): udhcpc: lease of {ip} obtained from 192.168.1.1, lease time 43200"
                ),
            ),
        ]);
        let count = lines.len() as i64;
        for (i, (priority, line)) in lines.into_iter().enumerate() {
            at.logd
                .push(booted + (i as i64) * BOOT_MS / count, priority, line);
        }
        out.push(Happening {
            node,
            at: self.now(),
            what: What::Up,
        });
        self.node_lease(node, out);
    }

    fn lease_due(&self, node: usize) -> bool {
        node != 0 && self.now_ms() - self.nodes[node].lease_ms > LEASE_MS / 2
    }

    /// The gateway acks access point `node`'s own lease.
    fn node_lease(&mut self, node: usize, out: &mut Vec<Happening>) {
        let at = self.now_ms();
        let (mac, ip, name) = {
            let node = &mut self.nodes[node];
            node.lease_ms = at;
            (node.mac, node.ip, node.name.clone())
        };
        self.log(
            0,
            at,
            30,
            format!("dnsmasq-dhcp[2092]: DHCPACK(br-lan) {ip} {mac} {name}"),
        );
        out.push(Happening {
            node: 0,
            at: self.now(),
            what: What::Lease {
                mac,
                ip,
                name: Some(name),
            },
        });
    }

    fn routine_log(&mut self, node: usize) {
        let gateway = self.nodes[node].gateway;
        let lines: Vec<_> = ROUTINE_LOGS
            .iter()
            .filter(|(for_gateway, _, _)| *for_gateway == gateway)
            .collect();
        let (_, priority, line) = lines[self.rng.below(lines.len() as u64) as usize];
        let line = line.replace("192.168.1.%ip", &self.nodes[node].ip.to_string());
        let at = self.now_ms() + self.rng.below(1000) as i64;
        self.log(node, at, *priority, line);
    }
}
