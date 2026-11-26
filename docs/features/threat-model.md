# Threat model

Read this when changing anything that listens, parses input from outside the controller, holds secrets, or acts on nodes. The mitigations themselves are specified in features/security.md; this file says what they defend, against whom, and what is still to check. M9.3 (D-150): a1 wrote this model; a2 reviews each surface's code against it and fixes what it finds (a2a: surfaces 1–2; a2b: 3–5 and node hardening; a2c: 6–8); b fuzzes the parsers marked *fuzz*.

## Scope

uniwrt runs on a home server inside the network it manages. It is **root on every adopted router** (it holds their SSH access and root passwords), so a compromised controller is a compromised network. The model is a home LAN: no internet exposure of the controller is supported (put it behind a VPN such as the gateway's WireGuard or Tailscale, features/network.md).

**Assets**, most valuable first:

1. **Control of the nodes:** the controller's SSH key, the pinned host keys, node root passwords; and the apply path (whatever reaches `txn` runs as root on a router).
2. **Network availability:** directive 8; a bad apply, a reboot storm or a firmware image can take the home offline.
3. **Secrets** in the vault (features/security.md, Secrets vault): Wi-Fi, mesh, PPPoE, WireGuard and Tailscale keys, notifiers' credentials, GitHub tokens, the local CA's key, the master key itself.
4. **Admin sessions and passwords.**
5. **Private data:** who is home (clients, roaming, presence), DNS queries per client, floor plans, logs, diagnostics bundles and packet captures.
6. **Integrity of what nodes install:** firmware images, packages from the feed mirror, the agent bundle.

**Out of scope:** an attacker with root on the controller's host or physical access to a node; a malicious admin (admins are root by design; their actions are audited); denial of service by flooding the LAN; side channels.

## Adversaries

| Id | Who | Can |
|---|---|---|
| A1 | A device on any LAN or VLAN: a guest, an IoT gadget, malware on a laptop | reach the controller's listeners if the firewall lets it; spoof its source address over UDP; answer ARP/ND, DHCP and mDNS; run rogue SSH servers |
| A2 | A radio neighbour outside the home | put any bytes in SSIDs, beacons and probe requests that the nodes scan and the UI shows; send hostile 802.11 frames to the nodes (the kernel's and hostapd's problem, not ours) |
| A3 | A compromised or hostile node (a router with a vulnerable service, or a device swapped in) | answer every agent op, collect set, file read and exec with any bytes; send syslog; serve a terminal; try to reach other nodes through the controller |
| A4 | A viewer account, or a stolen viewer session | every read the API allows viewers |
| A5 | An upstream gone bad: a feed, ASU, GitHub, a blocklist URL, a notifier endpoint, or a network attacker between them and the controller | answer the controller's outbound requests with any bytes, sizes and redirects |
| A6 | A thief of a backup, an export or the data volume | read every byte at rest |
| A7 | A malicious web page in the admin's browser | send cross-site requests and WebSocket upgrades to the controller's origin; frame it |

## Entry points

Everything that accepts bytes the controller didn't make. *Auth* is what a request needs; *fuzz* marks the parsers M9.3b fuzzes.

| Entry point | Reached by | Auth | Bounds (features/security.md and the specs) |
|---|---|---|---|
| HTTPS `:8443` `/api/v1/*` | A1, A4, A7 | session cookie, CSRF header and `Origin`; admin or viewer per route | body limits per route; login throttling; problem+json errors |
| `/api/v1/auth/setup`, `/auth/login` | A1 | none (setup only while no user exists, for 10 minutes) | throttled per client and per username; Argon2id two at a time |
| `/api/v1/ws` | A1, A7 | session, `Origin` | session checked every minute; at most 10 deltas/s per topic |
| `/api/v1/ztp/hello` | A1 | none (unauthenticated by design, D-102) | 4 KiB; 20 per 10 minutes per client; checked fields (*fuzz*) |
| `/b/{token}` (the one-liner's script) | A1 | a single-use 15-minute token | its SHA-256 is pinned in the command (D-103) |
| `/feeds/{host}/{path}` (the mirror) | A1, A3 | none (public packages) | only hosts the fleet's feeds name, only index files and files an index lists (D-147) |
| `/api/v1/tls/ca.crt`, `/api/v1/health` | A1 | none | public data |
| HTTP `:8080` | A1 | as HTTPS, after the redirect | trusted proxies only from `--trusted-proxies`, whose `X-Forwarded-*` count |
| Syslog `:5514` UDP and TCP | A1 (spoofable over UDP), A3 | none | per-sender rate, the query log's own rate and half the queue (*fuzz*: the syslog line, dnsmasq's query lines) |
| iperf3 `:5201` | A3 | bound only during a test, to that node's address (D-142) | the iperf3 protocol (*fuzz*: its control messages) |
| SSH sessions to nodes | A3 (and A1 impersonating a node) | host key pinned at adoption; ZTP hellos' keys shown for approval | the agent's JSON lines, collect sets and pushes, exec output, `read_file`, the UCI read, backups, captures (*fuzz*: the line decoder, every set's parser, UCI) |
| Discovery and probes | A1 | admin or viewer starts a probe | the probe's SSH banner and host key; mDNS, DHCP leases, neighbour tables (*fuzz*: the parsers of leases and neighbours) |
| Outbound HTTPS | A5 | the embedded CA roots; GitHub tokens for private sources | per source: blocklists 32 MiB each, feeds by index hashes, images by the ASU/downloads hashes, the controller's own releases by Sigstore (D-123) (*fuzz*: feed indexes, blocklists, the ASU and GitHub JSON) |
| Imports | admin | session | brownfield UCI import (*fuzz*: `uci`, `import`), the site file (age, D-130), controller backups (age), floor-plan images, firmware uploads |

## Threats by surface

Each line: the threat (adversary), how it is met today, and what a2 checks in the code (☐) or b fuzzes.

### 1. The API and the UI

Reviewed in a2a (2026-11-26). ☑ marks a checked item (with what was found or fixed), ⚠ a residual risk.

- **Session theft or forgery (A1, A7).** `__Host-` cookie, `HttpOnly`, `Secure`, `SameSite=Strict`; hashed at rest; CSRF header and `Origin`; WebSocket `Origin`. ☑ the CSRF check is a layer on the whole API router (fallbacks and uploads included); GET, HEAD, OPTIONS and CONNECT pass it, and the two WebSocket upgrades (`v1/ws`, the terminal) check `Origin` themselves; the hub's messages only subscribe (reads), and the terminal's carry the admin's keystrokes. ☑ behind a trusted proxy, `Origin` is compared with the proxy's `X-Forwarded-Proto` and `-Host` (`server/src/http/plain.rs`); a client's own headers count only from `--trusted-proxies`. ⚠ DNS rebinding gets past the `Origin` check (the attacker's name is in both `Origin` and `Host`) but brings no cookie, and the certificate doesn't match the attacker's name.
- **Privilege escalation by a viewer (A4).** ☑ roles are enforced **at the router** (`api/src/auth/roles.rs`): every route needs a session unless it is public, every state-changing route is for admins unless `VIEWER_WRITES` names it (saved log searches, the topology's pinned layout), and `ADMIN_READS` lists the reads that are for admins (archives, the bundle, captures, the terminal, the apply plan, ZTP's scripts and inbox); handlers keep their own checks. `roles_tests.rs` reads every `.route(…)` from the source and drives each through the router as a viewer and without a session. ☑ secrets: revealing a root password or a site secret, a WireGuard peer's config and the secrets' export are admin writes; the UCI read stays a viewer read (D-143; masked by name). ☑ found: **the adoption probe was open to viewers** (a LAN scanner from the controller's address): it is an admin write now. ⚠ viewers share saved searches and the pinned layout with admins (either may delete them).
- **Password guessing (A1).** Throttling and lockout, Argon2id, no default credentials, setup's 10-minute window. ☑ throttles are keyed by the client the listener resolved (behind a trusted proxy, the forwarded client) and by username, each map bounded; ☑ an unknown user's login verifies a dummy hash computed at start.
- **Cross-site scripting from hostile data (A2, A3, A1).** SSIDs, host names, DHCP options, log lines, vendor strings and UCI values reach the UI. Solid escapes text; the CSP forbids inline script and style, which also blocks `javascript:` URLs. ☑ no `innerHTML` or similar sink in `web/src`; links built from data are app paths or URLs from GitHub, ASU and the embedded presets; ☑ found: **the Tailscale login URL comes from the gateway (A3) and is linked**: the API refuses one that isn't an `https://` link without spaces or control characters (502). ☑ floor plans are PNG or WebP only (sniffed, never decoded on the controller). ☑ the terminal (xterm.js 6): OSC 8 links with a scheme other than http(s) are ignored, a link asks before it opens and nulls the opener; no clipboard addon (OSC 52 does nothing); the title isn't used.
- **Resource exhaustion (A1, A3).** ☑ axum's 2 MB body limit applies to every extractor, raw bodies included, unless a route sets its own (uploads: firmware 256 MiB, restore 8 GiB, floor images; the hello 4 KiB). ☑ page sizes are capped (200–1,000), the doctor's hours ≤ 168, metric widths from a fixed set, `since` clamped to retention. ☑ found: **a controller backup could unpack without bound** (a gzip bomb, uploadable without a session while setup is open): the database unpacks to at most 64× the bytes read plus 64 MiB, and at most 1 MiB of padding follows the last entry (`store/src/controller_backup/stage.rs`). The site file's age is under the 2 MB body limit; nothing else decompresses uploads.
- **Path traversal (A1).** ☑ the UI's assets are an embedded table (no file system); `/feeds` serves only a planned feed's exact path and a file name of OpenWrt's characters (`.` and `..` refused; feed paths are checked the same way when parsed); floor plans, node backups and jobs are numeric ids; controller backups' names must parse as one.
- **Reaching the controller at all (A1).** ☑ guest and isolated networks' zones reject input and forward only as the policy matrix allows; APs hold an address only on the management network. ⚠ found: **node hardening is specified (features/security.md) but not rendered**: trusted LANs reach the gateway's SSH, with password logins on; a2b renders it (D-150).
- **Clickjacking (A7).** `frame-ancestors 'none'` and `X-Frame-Options: DENY`.

### 2. Unauthenticated endpoints

- **Rogue adoption (A1).** A hello only lists a device in the inbox; adoption needs an admin's approval or a pre-approval bound to board and MAC (D-102), and shows the host key's fingerprint. ☑ the inbox keeps 64 devices (the least recently seen dropped), fields of 128 printable characters, hellos of 4 KiB, 20 per 10 minutes per client (10,000 clients tracked); ☑ the adoption checks the device's own MAC and board (read over SSH) against the hello's claim, and a reset's approval its board. ⚠ a pre-approval trusts whoever first claims the MAC (D-102): a device on the LAN that spoofs it before the real one is adopted in its place, which the jobs list and the device's fingerprint show.
- **Swapping the one-liner's script (A1).** The SHA-256 is pinned (D-103). ☑ the token is 128 random bits, fetched once (Waiting → Fetched), used once (→ Used), the fetch within 15 minutes and the hello 15 minutes after it, at most 16 held, all gone at restart; the script holds the controller's public key and addresses, no secret. ⚠ the fetch's TLS is unchecked, so an active man in the middle could read the token and race the device's hello.
- **The mirror as a proxy or oracle (A1).** Only named hosts and listed files. ☑ normalization as in §1's path traversal; a pull-through fetches only a file the feed's index lists, checked by its hash, so the downloads are bounded by what the fleet's feeds hold. ⚠ the hosts are the ones the nodes' feed lines name, so a hostile node (A3) can point the mirror at a LAN host's path (fetched, hash-checked, never echoed unless it matches).

### 3. Syslog and iperf3

- **Forged logs (A1).** Over UDP the source address is spoofable, so a log line proves nothing; logs feed alerts, the doctor and DNS statistics. ☐ lines from addresses that aren't adopted nodes are dropped (or kept apart) before the engine stores or acts on them; ☐ no automatic action trusts a log line alone; ☐ the query log's statistics can't be poisoned to block or allow a domain without an admin.
- **Parser crashes (A1, A3).** *Fuzz* the RFC 3164/5424 parser, the annotation patterns and `dns_queries`.
- **iperf3 (A3).** Bound to one node for one test. *Fuzz* the control channel's JSON and lengths.

### 4. Node sessions

- **Impersonating a node (A1).** Host keys are pinned at adoption and a change quarantines the node. ☐ every connection path (agent, exec, terminal, jump host, re-adoption, replacement) checks the pin; ☐ password logins happen only at adoption.
- **Command injection into a node (A1 via names, A4 via inputs, A3 via its own answers).** Ops take argv arrays and JSON; commands are quoted for ash; D-036 has the one exception. ☐ every place that builds a shell string (the quoting function, the per-command feed wrapper, `txn`, `probe.uc`, the diagnostics' targets, packet capture filters, ZTP and the one-liner scripts); ☐ values the UCI serializer writes (newlines, quotes, NUL).
- **A hostile node attacking the controller (A3).** Every answer is untrusted: ☐ size limits on lines, sets, exec output, `read_file`, the UCI read, backups and captures; ☐ no panics on malformed input (*fuzz* every set's parser and the line decoder); ☐ a node can't write outside its own facts, files and logs (its id, MAC and addresses can't collide with another node's); ☐ the terminal can't reach other channels; ☐ `direct-tcpip` through a node only to targets the controller chose.
- **A hostile node attacking other nodes (A3).** Nodes never talk to each other through the controller; a node's facts feed the planner (RF, topology, steering). ☐ bogus facts can only degrade the plans of what they describe, and validation refuses impossible values (channels, powers, MTUs).

### 5. The agent and the transaction guard (on the node)

- **Tampering with the agent (A1 on the node's LAN can't; a local process could).** `/tmp/uniwrt` is root's and private, the install checks every file's hash, the hello's hash must match. ☐ the install script's handling of symlinks and races (`.new`, the `flock`); ☐ files written under `/tmp/uniwrt/run` and secrets passed as root-only files (Tailscale's auth key) are created with safe modes before data is written.
- **Bricking (directive 8).** Two-phase transactions with a dead-man timer and a boot-time rollback (D-069). ☐ the guard rolls back when the controller dies mid-apply, when the node reboots mid-apply, and when `/etc/uniwrt/journal` is full or corrupt.

### 6. Outbound fetches and updates

- **Malicious content (A5).** TLS against the embedded roots; size caps; feeds and images checked by their hashes and signatures; the controller's own updates by Sigstore (D-123). ☐ every outbound client verifies certificates (no `danger_accept_invalid_certs`) and caps redirects and sizes; ☐ firmware images are checked before `sysupgrade` runs; ☐ blocklist lines can't inject dnsmasq directives (only `server=/domain/` lines are written); ☐ GitHub sources' assets are checked as the release names them.
- **Server-side requests chosen by an admin.** Webhooks, ntfy, SMTP, custom blocklists and GitHub URLs can name LAN hosts; admins are trusted, so this is accepted. ☐ viewers can't trigger them with their own URLs; ☐ responses aren't echoed back verbatim (they could hold a LAN service's data).

### 7. Data at rest and backups

- **A stolen volume or backup (A6).** Secrets sealed under the master key; sessions hashed; controller backups and site exports under an age passphrase; the key file 0600 and movable to a Docker secret. ☐ nothing secret is logged (tracing fields, job histories, events, the audit log's details, the diagnostics bundle); ☐ files the controller writes under `/data` get 0600/0700; ☐ `UNIWRT_MASTER_KEY` in the environment is readable through `/proc` by the same UID only (documented).

### 8. Supply chain and the container

- **A poisoned dependency or build (A5).** Lockfiles, exact pins, cargo-deny, osv-scanner, Dependabot, zizmor over the workflows, CodeQL, cargo-auditable, SBOM, provenance and cosign signatures (features/security.md, Supply chain). The container runs as 65532 with a read-only root and no shell.
- **The release path.** ☐ the self-update verifies the Sigstore bundle's identity (this repository's `ci.yml` on `main`) and the digest before replacing anything.
