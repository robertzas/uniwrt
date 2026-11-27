# Install and first start

uniwrt is one program: a controller that runs on a computer in your home, such as a home server, a NAS or a Raspberry Pi, and manages your OpenWrt routers over SSH. You run it as a container or as a single static binary. Neither needs anything installed on your routers beforehand. This chapter covers installing it, creating the first admin, trusting its certificate and keeping its keys safe.

It runs on any Linux host with an x86_64, x86 (i686), arm32 (armv7) or arm64 CPU. It manages routers running OpenWrt 25.12, and 24.10 on a best-effort basis.

## Run the container

The image is `ghcr.io/robertzas/uniwrt`. It has tags for each release (`X.Y.Z` and `X.Y`), plus `latest`. It holds the program alone, with no shell or other tools. It runs as user and group 65532, needs no extra capabilities and works with a read-only root filesystem (`--read-only`). Everything it keeps goes into `/data`.

### With host networking (recommended)

```sh
docker run -d --name uniwrt --restart unless-stopped \
  --network host -v uniwrt-data:/data \
  ghcr.io/robertzas/uniwrt:latest
```

Host networking is the simplest setup. The controller sees your LAN directly, which makes device discovery and log collection straightforward. It also follows the host when the network is renumbered.

Then open `https://<host>:8443` and [create the admin account](#create-the-admin-account).

### With bridge networking

Publish the ports, and name the host's address for the certificate on the first run:

```sh
docker run -d --name uniwrt --restart unless-stopped \
  -p 8443:8443 -p 8080:8080 -p 5514:5514/tcp -p 5514:5514/udp \
  -e UNIWRT_TLS_NAMES=192.168.1.10 \
  -v uniwrt-data:/data \
  ghcr.io/robertzas/uniwrt:latest
```

Replace `192.168.1.10` with the address (or a DNS name) you use to reach the host. Inside a bridged container the controller can't see the host's address, so without this setting the certificate doesn't cover it. Set it before the first start: see [Names the certificate covers](#names-the-certificate-covers).

- Publish 8443 as 8443. The HTTP listener redirects to the HTTPS listener's own port, so a different outside port breaks the redirect.
- Throughput tests against the controller also need port 5201 (`-p 5201:5201`). See [Ports](#ports).

### With Docker Compose

```yaml
services:
  uniwrt:
    image: ghcr.io/robertzas/uniwrt:latest
    restart: unless-stopped
    network_mode: host
    volumes:
      - uniwrt-data:/data
volumes:
  uniwrt-data:
```

Add an `environment:` list for any of the [settings](#settings-reference).

Keep a restart policy (`--restart unless-stopped`, or `restart:` in Compose). Restoring a backup from the web UI stops the controller, and the restart policy starts it again.

### Ports

| Port | Used for |
|---|---|
| 8443/tcp | The web UI and the API, over HTTPS. Your devices also reach the controller here (new devices calling home, and the offline package mirror). |
| 8080/tcp | Plain HTTP. It redirects to HTTPS, except for the health check and for [reverse proxies](#behind-a-reverse-proxy) you trust. |
| 5514/tcp and udp | The syslog receiver, for routers and other devices that send their logs. |
| 5201/tcp | The controller's iperf3 server. It listens only while a throughput test with the controller runs, and only for that device. |

If the host has a firewall, let your routers reach 8443, 5514 and 5201.

### The data volume

`/data` holds the database, the vault's master key, the backups, the firmware cache and the package mirror. A new named volume (as above) gets the right owner and mode by itself.

To keep the data in a host directory instead, make it writable by UID 65532 first:

```sh
mkdir -p /srv/uniwrt && chown 65532:65532 /srv/uniwrt && chmod 700 /srv/uniwrt
docker run ... -v /srv/uniwrt:/data ...
```

Every file the controller writes is readable by its own user only. It warns in its log if others can open the data directory.

> [!WARNING]
> `/data/master.key` opens every secret the controller keeps: device passwords, Wi-Fi passphrases, VPN keys. Anyone with a copy of the volume has them. See [The master key](#the-master-key).

## Run the static binary

Each [GitHub release](https://github.com/robertzas/uniwrt/releases) has a tarball for each CPU, `uniwrt-<version>-<target>.tar.gz`, and a `SHA256SUMS` file:

| CPU | Target |
|---|---|
| x86_64 | `x86_64-unknown-linux-musl` |
| x86 (32-bit) | `i686-unknown-linux-musl` |
| arm32 | `armv7-unknown-linux-musleabihf` |
| arm64 | `aarch64-unknown-linux-musl` |

The binary is fully static: the web UI, the CA roots, the time zones and the SSH client are built in.

1. Download the tarball for your CPU and `SHA256SUMS`, and [verify them](#verify-a-release).
2. Unpack it: `tar xzf uniwrt-<version>-<target>.tar.gz`.
3. Start it with a data directory: `./uniwrt serve --data-dir /var/lib/uniwrt`.

Without `--data-dir`, the binary uses `./data` in the current directory. It needs no root: its default ports are above 1024.

### Run it as a service

An example systemd unit, for a `uniwrt` user that owns `/var/lib/uniwrt`:

```ini
[Unit]
Description=uniwrt controller
After=network-online.target
Wants=network-online.target

[Service]
User=uniwrt
ExecStart=/usr/local/bin/uniwrt serve --data-dir /var/lib/uniwrt
Restart=always

[Install]
WantedBy=multi-user.target
```

`Restart=always` matters for the same reason as a container's restart policy: a restore stops the controller and expects to be started again.

The controller can update itself from its GitHub releases. It keeps the newer binary in `<data dir>/bin`, so the data directory must allow running programs (not mounted `noexec`). If a package manager owns the binary, start it with `--self-upgrade off`. See [Updates](updates.md).

## Create the admin account

uniwrt has no default password. A controller that starts with no users opens a setup window for 10 minutes. Its log says so: `no users yet: create the admin in the web UI within 10 minutes`.

1. Open `https://<host>:8443`. Your browser warns about the certificate until you [trust the local CA](#trust-the-controllers-certificate); you can continue past the warning this once.
2. On **Create the admin account**, keep or change the **Username** (`admin` by default).
3. Type a **Password** of at least 12 characters, and type it again in **Confirm password**. A long passphrase works best.
4. Click **Create account**. You are signed in, and the first-run wizard starts. [Getting started](getting-started.md) continues from there.

The form shows how many minutes setup stays open. If the window closes first, you see **Setup is closed**: restart the controller (`docker restart uniwrt`), then click **Check again**. The window exists so that nobody else on your network can claim a forgotten controller.

Moving from another controller? Under the form, **Moving a controller? Restore a backup instead.** restores a backup in place of creating an account. See [Backups](settings.md#backups).

## Trust the controller's certificate

On its first start the controller creates its own small certificate authority (the local CA) and uses it to issue its HTTPS certificate. Browsers don't know this CA, so they warn until you trust it on each device you use. Once you do, the warnings are gone for good, and the certificate renews itself.

The CA is limited: it can sign only local names (`localhost`, and names under `lan`, `home.arpa`, `local` and `internal`), private and link-local addresses, and the names configured on the first start. A device that trusts it can't be fooled about any other site, even by someone who stole the CA's key.

### Download and check the CA

1. Download it from `https://<host>:8443/api/v1/tls/ca.crt`.
2. Compare its SHA-256 fingerprint with the one the controller prints. The log line `HTTPS certificate` at each start shows it as `ca_sha256`, and `https://<host>:8443/api/v1/tls` shows it too, with the names the certificate covers.
3. On a computer, `openssl x509 -in ca.crt -noout -fingerprint -sha256` prints the fingerprint in the same format.

### Install it on your devices

Import `ca.crt` as a trusted root (CA) certificate:

- Windows: open the file, choose Install Certificate, and put it in Trusted Root Certification Authorities.
- macOS: add it to the System keychain in Keychain Access, then set it to Always Trust.
- iPhone and iPad: open the file to install the profile, then turn on full trust for it under Settings, General, About, Certificate Trust Settings.
- Android: install it as a CA certificate under the security settings.
- Linux: add it to the system store (for example `/usr/local/share/ca-certificates/`, then `update-ca-certificates`). Firefox keeps its own store: import it under its certificate settings.

### Names the certificate covers

The certificate covers `localhost`, `uniwrt.lan`, the host's name and `<host name>.lan`, the loopback addresses, the HTTPS listen address, and the host's private addresses. It is checked once a day and reissued when the host's addresses change, 30 days before it expires, or at a restart.

For any other name or address that you type in the browser, add it with `UNIWRT_TLS_NAMES` (comma-separated, no wildcards). Do this before the first start: the CA is created then, and it can't sign other names later. Names under `lan`, `home.arpa`, `local` or `internal` and private addresses work at any time. A name the CA can't sign is left out of the certificate, with a warning in the log.

> [!TIP]
> If your gateway's DNS knows the host (OpenWrt's local domain is `lan`), open the controller as `https://<host name>.lan:8443`. That name keeps working when the network is renumbered.

## The master key

Secrets (device root passwords, the controller's SSH key, Wi-Fi passphrases, VPN keys, notification credentials) are stored encrypted. The key that opens them is created on the first start in `<data dir>/master.key`. The log says `generated the vault's master key; back it up apart from the database`.

- Back up `master.key` separately from the data. Without it, the secrets are lost, and the controller refuses to start with a database whose secrets it can't open.
- To keep the key out of the data volume, so that a copy of the volume alone opens nothing, put it in a file elsewhere and point `UNIWRT_MASTER_KEY_FILE` (or `--master-key-file`) at it, for example a Docker secret at `/run/secrets/master.key`.
- The key is 64 hexadecimal digits. To make one yourself: `openssl rand -hex 32 > master.key`, then `chmod 600 master.key`.
- `UNIWRT_MASTER_KEY` can hold the key itself. Prefer the file: anyone with access to Docker can read a container's environment with `docker inspect`.

If the file is missing on a start, a new key is created only while nothing is stored under the old one. Otherwise the start fails with `the master key … is missing`: put the file back, or set `UNIWRT_MASTER_KEY`.

## Backups

Once an admin sets a backup passphrase, the controller backs itself up every night into `<data dir>/backups` and keeps the newest 7. Each file (`uniwrt-backup-….tar.gz.age`) is encrypted with the passphrase and holds the database and the master key, so it alone restores everything. Set the passphrase, take, download and restore backups under [Settings, Backups](settings.md#backups).

- To copy each backup off the host (to a NAS mount, say), add another volume and set `UNIWRT_BACKUP_DIR`, for example `-v /mnt/nas/uniwrt:/backups -e UNIWRT_BACKUP_DIR=/backups`. The last 7 are kept there too.
- To restore from the command line, run `uniwrt restore FILE` with the same data directory, then start the controller: the next start puts the backup in place. The passphrase comes from `UNIWRT_BACKUP_PASSPHRASE` or is asked for. With Docker, while the controller is stopped:

```sh
docker run --rm -i -v uniwrt-data:/data -v "$PWD":/in:ro \
  ghcr.io/robertzas/uniwrt:latest restore /in/uniwrt-backup-<time>.tar.gz.age
```

A restore keeps what it replaces in `<data dir>/restore/previous` until the next restore.

## Behind a reverse proxy

A reverse proxy that terminates TLS (Caddy, nginx, Traefik) should forward to the HTTP port, 8080, not to 8443.

1. Point the proxy at `http://<host>:8080`. Let it pass WebSocket upgrades: the live views and the terminal use them.
2. Have it send `X-Forwarded-For`, `X-Forwarded-Proto` (`https`) and `X-Forwarded-Host` (or the original `Host`).
3. Start the controller with the proxy's address in `UNIWRT_TRUSTED_PROXIES`, as networks, comma-separated: `-e UNIWRT_TRUSTED_PROXIES=127.0.0.1/32`, or `172.16.0.0/12` for a proxy in another container.

Only requests from those networks reach the application over plain HTTP; everyone else is redirected to HTTPS. The forwarded headers count only from them, so sign-in limits and the audit log see the real client.

## Settings reference

Every flag of `uniwrt serve` has an environment variable. Lists are comma-separated.

| Flag | Environment variable | Default | What it does |
|---|---|---|---|
| `--https-listen` | `UNIWRT_HTTPS_LISTEN` | `0.0.0.0:8443` | Address and port of the web UI and API. |
| `--http-listen` | `UNIWRT_HTTP_LISTEN` | `0.0.0.0:8080` | Address and port of plain HTTP (redirects, health check, trusted proxies). |
| `--tls-names` | `UNIWRT_TLS_NAMES` | none | More DNS names and IP addresses for the certificate. Set before the first start. |
| `--trusted-proxies` | `UNIWRT_TRUSTED_PROXIES` | none | Networks of reverse proxies that forward to the HTTP port. |
| `--syslog-listen` | `UNIWRT_SYSLOG_LISTEN` | `0.0.0.0:5514` | The syslog receiver (UDP and TCP), or `off`. |
| `--iperf3-listen` | `UNIWRT_IPERF3_LISTEN` | `[::]:5201` | The iperf3 server for throughput tests with the controller, or `off`. Port `0` picks a free port for each test. |
| `--asu-url` | `UNIWRT_ASU_URL` | `https://sysupgrade.openwrt.org` | The Attended Sysupgrade service that builds images for new devices, or `off`. Nothing secret is sent to it. |
| `--data-dir` | `UNIWRT_DATA_DIR` | `./data` (`/data` in the image) | Database, keys, backups, firmware and package cache. |
| `--backup-dir` | `UNIWRT_BACKUP_DIR` | none | Another directory each controller backup is copied to. |
| `--master-key-file` | `UNIWRT_MASTER_KEY_FILE` | `<data dir>/master.key` | Where the master key file is. |
| none | `UNIWRT_MASTER_KEY` | none | The master key itself (64 hex digits), instead of the file. |
| `--self-upgrade` | `UNIWRT_SELF_UPGRADE` | `on` | Whether the controller may update itself. `off` when a package manager owns the binary. |
| `--log` | `UNIWRT_LOG` | `info` | Log level, for example `debug` or `info,uniwrt=debug`. Logs go to standard error (`docker logs uniwrt`). |
| `--demo` | `UNIWRT_DEMO` | off | [Demo mode](#try-the-demo). |
| `--demo-seed` | `UNIWRT_DEMO_SEED` | fixed | The demo's seed: the same seed gives the same fleet. |
| `--demo-access-points` | `UNIWRT_DEMO_ACCESS_POINTS` | 3 | Access points in the demo (1 to 250). |
| `--demo-clients` | `UNIWRT_DEMO_CLIENTS` | 30 | Clients in the demo (0 to 10,000; 12 with `--demo-factory`). |
| `--demo-factory` | `UNIWRT_DEMO_FACTORY` | off | The demo as a brand-new network. |

`UNIWRT_BACKUP_PASSPHRASE` gives `uniwrt restore` its passphrase. `TOKIO_WORKER_THREADS` overrides the number of worker threads (one per core, at most 4).

### Commands

- `uniwrt serve` (or `uniwrt` alone) runs the controller.
- `uniwrt healthcheck` exits 0 if the controller answers its health check. The image's Docker health check runs it.
- `uniwrt version` prints the version and build (`--json` for scripts).
- `uniwrt restore FILE` stages a controller backup for the next start.

## Try the demo

Demo mode simulates a gateway, 3 mesh access points and 30 clients that roam, come and go, and now and then restart an access point. It never touches your real data: it keeps its own database in a temporary directory and deletes it at exit.

```sh
docker run --rm -p 8443:8443 -e UNIWRT_DEMO=1 ghcr.io/robertzas/uniwrt:latest
```

With the binary: `uniwrt serve --demo`.

- Sign in as `demo`, password `demo`.
- The last access point waits under **Devices** for you to adopt it; its root password is `demo`. A factory-default access point also shows up on the gateway's adoption port a little after the start.
- `--demo-factory` (`-e UNIWRT_DEMO_FACTORY=1`) simulates a factory-default router alone, with no user. Create the admin, and the first-run wizard sets the router up as on a new network.
- The demo's syslog receiver is off unless you set `--syslog-listen`, and it never updates itself.

## Verify a release

Every image is signed with cosign (keyless) and carries SBOM and provenance attestations. Verify it with cosign v3 or later (v2 reports "no signatures found"):

```sh
cosign verify ghcr.io/robertzas/uniwrt:<version> \
  --certificate-identity https://github.com/robertzas/uniwrt/.github/workflows/ci.yml@refs/heads/main \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

Every tarball has a build provenance attestation and a line in `SHA256SUMS`:

```sh
gh attestation verify uniwrt-<version>-<target>.tar.gz --repo robertzas/uniwrt
sha256sum -c --ignore-missing SHA256SUMS
```

## Uninstall or start over

Before you remove the controller, decide what happens to your routers. They keep running with the last configuration it applied, but they also keep its SSH key, and a device that had no root password at adoption has a random one that only the controller's vault knows.

1. For each device you want to keep using without uniwrt, open it under **Devices**, then its **Config** tab, and click **Decommission…** on the **Replace or remove** card. uniwrt removes itself from the device; note down the root password it shows first, or tick **Also reset it to its defaults**.
2. Stop and remove the container: `docker rm -f uniwrt`.
3. Delete its data: `docker volume rm uniwrt-data` (or the host directory). This deletes the database, the master key and the backups in it, for good.

To start over with an empty controller instead, do steps 2 and 3, then [run the container](#run-the-container) again. A new controller can adopt devices again only with their current root passwords, so reset them or note the passwords first. Keep a [backup](#backups) if you might want the old state back.
