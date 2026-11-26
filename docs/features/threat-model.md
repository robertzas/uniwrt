# Threat model

Read this when changing anything that listens, parses input from outside the controller, holds secrets, or acts on nodes. The mitigations themselves are specified in features/security.md; this file says what they defend, against whom, and what is still to check. M9.3 (D-150): a1 wrote this model; a2 reviews each surface's code against it and fixes what it finds; b fuzzes the parsers marked *fuzz*.

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

- **Session theft or forgery (A1, A7).** `__Host-` cookie, `HttpOnly`, `Secure`, `SameSite=Strict`; hashed at rest; CSRF header and `Origin`; WebSocket `Origin`. ☐ every state-changing route is behind the CSRF check, including uploads and the WebSocket's messages; ☐ `Origin` comparison with a trusted proxy's forwarded host.
- **Privilege escalation by a viewer (A4).** Roles per route. ☐ a route table test: every mutating route and every secret read refuses a viewer; ☐ reads that reveal secrets or private data to viewers (root-password reveal, secrets reveal, the bundle, captures, the terminal, the UCI read's masking); ☐ the adoption probe, which viewers may run, makes the controller connect to any host and port (a LAN scanner with the controller's address): limit it to admins or to SSH on private addresses.
- **Password guessing (A1).** Throttling and lockout, Argon2id, no default credentials, setup's 10-minute window. ☐ the throttle's keying behind a trusted proxy; ☐ timing of unknown users.
- **Cross-site scripting from hostile data (A2, A3, A1).** SSIDs, host names, DHCP options, log lines, vendor strings and UCI values reach the UI. Solid escapes text; the CSP forbids inline script and style. ☐ no `innerHTML`, no `href`/`src` built from untrusted values without a scheme check (`javascript:`), SVG floor plans served as images only; ☐ the terminal (xterm.js): OSC sequences a node can send (hyperlinks, clipboard writes, title), and the link handler.
- **Resource exhaustion (A1, A3).** Body limits on uploads; the bounded caches. ☐ a default body limit on every route without its own; ☐ limits on query parameters (page sizes, time ranges); ☐ decompression bombs in uploads and imports (age, gzip, tar).
- **Path traversal (A1).** ☐ the UI's asset fallback, `/feeds` (`..`, encoded slashes, absolute paths), floor-plan and backup ids.
- **Reaching the controller at all (A1).** The controller sits on a LAN the gateway renders. ☐ guest and isolated networks (features/network.md) can't reach the controller's address and ports through the rendered firewall; ☐ the management VLAN, when used, is the only one that reaches SSH on the nodes.
- **Clickjacking (A7).** `frame-ancestors 'none'` and `X-Frame-Options: DENY`.

### 2. Unauthenticated endpoints

- **Rogue adoption (A1).** A hello only lists a device in the inbox; adoption needs an admin's approval or a pre-approval bound to board and MAC (D-102), and shows the host key's fingerprint. ☐ the inbox's bounds (entries kept, their sizes); ☐ a pre-approval can't be consumed by a device with another board or MAC.
- **Swapping the one-liner's script (A1).** The SHA-256 is pinned (D-103). ☐ the token's entropy, single use and expiry; the script embeds no secret that a LAN observer could reuse (the TLS is unchecked).
- **The mirror as a proxy or oracle (A1).** Only named hosts and listed files. ☐ the request path's normalization; requests that would make the controller download (pull-through) are bounded in count and size.

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
