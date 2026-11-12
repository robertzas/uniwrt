//! The first-boot script of zero-touch images on OpenWrt (features/adoption.md, Path B; D-102),
//! run by `scripts/test-agent.sh` per image (`UNIWRT_ZTP_IMAGE`): a stand-in controller
//! (dnsmasq giving leases and `uniwrt.lan`, uhttpd over TLS answering the hello) and a device
//! booted with the script in `/etc/uci-defaults`, a second network giving it a WAN port. The
//! device must take a lease on its LAN bridge (its WAN port in it), serve no DHCP, trust the
//! controller's key, lock root's password, call home past an address that fails, and remove
//! its service once adopted.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    env,
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};

use uniwrt_model::{node::NodeRole, ztp::ZtpHello};
use uniwrt_node::{
    ControllerKey, HostKey,
    ztp::{Endpoint, first_boot},
};

const SUBNET: &str = "172.31.77.0/24";
const CONTROLLER_IP: &str = "172.31.77.2";

/// The stand-in controller: a certificate, the hello's CGI (keeps each body, which uclient-fetch
/// sends chunked, and answers `adopted`), dnsmasq, then uhttpd.
const CONTROLLER: &str = r#"
set -e
px5g selfsigned -newkey ec -pkeyopt ec_paramgen_curve:P-256 -days 2 -keyout /tmp/key -out /tmp/cert \
  -subj /CN=uniwrt.lan 2>/dev/null
mkdir -p /www/api/v1/ztp /tmp/hellos
cat >/www/api/v1/ztp/hello <<'CGI'
#!/bin/sh
n=$(ls /tmp/hellos | wc -l)
cat >"/tmp/hellos/$n.json"
printf 'Status: 200 OK\r\nContent-Type: application/json\r\n\r\n{"state":"adopted"}'
CGI
chmod 755 /www/api/v1/ztp/hello
dnsmasq --port=53 --no-resolv --address=/uniwrt.lan/172.31.77.2 \
  --dhcp-range=172.31.77.100,172.31.77.150,255.255.255.0,1h --dhcp-leasefile=/tmp/leases --pid-file= --user=root
exec uhttpd -f -s 0.0.0.0:443 -C /tmp/cert -K /tmp/key -h /www -x /api/v1/ztp
"#;

/// The device boots as OpenWrt does: its resolver follows `/tmp/resolv.conf` (Docker mounts its
/// own over `/etc/resolv.conf`; unmounting it needs `SYS_ADMIN`).
const DEVICE: &str = "umount /etc/resolv.conf && ln -sf /tmp/resolv.conf /etc/resolv.conf \
    && exec /sbin/init";

struct Lab {
    names: Vec<(&'static str, String)>,
}

impl Lab {
    fn new() -> Self {
        Self { names: Vec::new() }
    }

    fn keep(&mut self, kind: &'static str, name: &str) {
        self.names.push((kind, name.to_owned()));
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        for (kind, name) in self.names.iter().rev() {
            let args: &[&str] = if *kind == "container" {
                &["rm", "-f"]
            } else {
                &["network", "rm"]
            };
            let _ = Command::new("docker").args(args).arg(name).output();
        }
    }
}

fn docker(args: &[&str]) -> Output {
    let output = Command::new("docker").args(args).output().unwrap();
    assert!(
        output.status.success(),
        "docker {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn exec(container: &str, script: &str) -> Option<String> {
    let output = Command::new("docker")
        .args(["exec", container, "/bin/sh", "-c", script])
        .output()
        .unwrap();
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn wait_for(what: &str, limit: Duration, mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(start.elapsed() < limit, "{what} within {limit:?}");
        thread::sleep(Duration::from_millis(500));
    }
}

/// The stand-in controller, then the device booting `script` from `/etc/uci-defaults`, on
/// networks of their own: (the controller, the device).
fn boot(lab: &mut Lab, image: &str, script: &str) -> (String, String) {
    let tag = format!("uniwrt-ztp-{}", std::process::id());
    let (lan, wan) = (format!("{tag}-lan"), format!("{tag}-wan"));
    let (controller, device) = (format!("{tag}-controller"), format!("{tag}-device"));

    // Docker hands out .2–.63 only; dnsmasq leases .100–.150.
    docker(&[
        "network",
        "create",
        "--subnet",
        SUBNET,
        "--ip-range",
        "172.31.77.0/26",
        &lan,
    ]);
    lab.keep("network", &lan);
    docker(&["network", "create", "--internal", &wan]);
    lab.keep("network", &wan);
    docker(&[
        "run",
        "-d",
        "--network",
        &lan,
        "--ip",
        CONTROLLER_IP,
        "--cap-add",
        "NET_ADMIN",
        "--name",
        &controller,
        image,
        "/bin/sh",
        "-c",
        CONTROLLER,
    ]);
    lab.keep("container", &controller);

    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), script).unwrap();

    // eth0 on the LAN network, eth1 (the WAN port) on the other.
    docker(&[
        "create",
        "--network",
        &lan,
        "--cap-add",
        "NET_ADMIN",
        "--cap-add",
        "SYS_ADMIN",
        "--name",
        &device,
        image,
        "/bin/sh",
        "-c",
        DEVICE,
    ]);
    lab.keep("container", &device);
    docker(&["network", "connect", &wan, &device]);
    let target = format!("{device}:/etc/uci-defaults/99-uniwrt");
    docker(&["cp", file.path().to_str().unwrap(), &target]);
    docker(&["start", &device]);

    (controller, device)
}

/// A lease on the bridge with the WAN port in it, no DHCP served, only the controller's key
/// (`line`) let in, and root's password locked.
fn safe_to_plug_in(device: &str, line: &str) {
    // Safe to plug in anywhere: a lease on the bridge, the WAN port in it, no DHCP served.
    let uci = |key: &str| exec(device, &format!("uci -q get {key}"));
    assert_eq!(uci("network.lan.proto").as_deref(), Some("dhcp"));
    assert_eq!(uci("network.lan.ipaddr"), None);
    assert_eq!(uci("network.wan"), None);
    assert_eq!(uci("dhcp.lan.ignore").as_deref(), Some("1"));
    assert!(
        uci("network.@device[0].ports")
            .unwrap()
            .split(' ')
            .any(|port| port == "eth1"),
        "the WAN port joins the bridge"
    );
    let address = exec(device, "ip -4 -o addr show dev br-lan").unwrap();
    assert!(address.contains("inet 172.31.77.1"), "{address}");
    assert_eq!(
        exec(device, "grep -l dhcp-range /var/etc/dnsmasq.conf.*"),
        None,
        "no DHCP served"
    );

    // Only the controller's key gets in.
    let keys = exec(device, "cat /etc/dropbear/authorized_keys").unwrap();
    assert!(keys.lines().any(|l| l == line.trim()), "{keys}");
    let root = exec(device, "grep '^root:' /etc/shadow | cut -d: -f2").unwrap();
    assert!(
        root.starts_with('$'),
        "root has a random password: {root:?}"
    );
}

#[test]
#[ignore = "needs Docker and an OpenWrt rootfs image (scripts/test-agent.sh)"]
fn a_first_boot_calls_home_safely_and_stops_once_adopted() {
    let image = env::var("UNIWRT_ZTP_IMAGE").expect("UNIWRT_ZTP_IMAGE=<OpenWrt rootfs image>");
    let mut lab = Lab::new();
    let key = ControllerKey::generate().unwrap();
    let line = key.authorized_key().unwrap();
    let endpoints = [
        Endpoint {
            host: "nowhere.invalid".to_owned(),
            port: 443,
        },
        Endpoint {
            host: "uniwrt.lan".to_owned(),
            port: 443,
        },
    ];
    let script = first_boot(&line, &endpoints, NodeRole::Ap).unwrap();
    let (controller, device) = boot(&mut lab, &image, &script);

    wait_for("the hello", Duration::from_secs(120), || {
        exec(&controller, "test -s /tmp/hellos/0.json").is_some()
    });
    let body = exec(&controller, "cat /tmp/hellos/0.json").unwrap();
    let hello: ZtpHello = serde_json::from_str(&body).unwrap();

    // What it said is what it is.
    let mac = exec(&device, "cat /sys/class/net/br-lan/address").unwrap();
    assert_eq!(hello.mac.to_string(), mac);
    let host_key = exec(
        &device,
        "dropbearkey -y -f /etc/dropbear/dropbear_ed25519_host_key | grep '^ssh-ed25519' | cut -d' ' -f1,2",
    )
    .unwrap();
    assert_eq!(hello.host_key, host_key);
    HostKey::parse(&hello.host_key).unwrap();
    let release = exec(&device, ". /etc/openwrt_release && echo $DISTRIB_RELEASE").unwrap();
    assert_eq!(hello.release, release);
    assert!(!hello.board_name.is_empty() && !hello.model.is_empty());
    assert_eq!(hello.role, Some(NodeRole::Ap));
    let suffix: String = mac.replace(':', "").chars().skip(8).collect();
    // A container keeps Docker's hostname (`/proc/sys` is read-only): the configured one counts.
    assert_ne!(hello.hostname, "");
    let hostname = exec(&device, "uci get system.@system[0].hostname");
    assert_eq!(hostname, Some(format!("uniwrt-{suffix}")));

    safe_to_plug_in(&device, &line);

    // `adopted`: the service removes itself and calls no more.
    wait_for("the service gone", Duration::from_secs(30), || {
        exec(
            &device,
            "test -e /etc/init.d/uniwrt-hello || test -e /usr/sbin/uniwrt-hello",
        )
        .is_none()
    });
    assert_eq!(exec(&device, "ls /etc/rc.d | grep uniwrt-hello"), None);
    thread::sleep(Duration::from_secs(3));
    assert_eq!(
        exec(&controller, "ls /tmp/hellos | grep -c json").as_deref(),
        Some("1")
    );
}
