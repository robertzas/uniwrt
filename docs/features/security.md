# Security

Read this when working on authentication, sessions, TLS, secrets, node hardening, the container, or supply chain and releases.
Milestones: M1.6 (TLS, vault), M1.6b (auth and sessions), M3.1 (vault for config secrets), M9.3 (review, fuzzing).

## Threat model (home LAN)

| Threat | Mitigation |
|---|---|
| A LAN attacker targets the controller UI/API | HTTPS; Argon2id passwords; login rate limiting and lockout; secure session cookies; CSRF defenses; no default credentials (first-run setup) |
| A stolen backup or data volume | Secrets encrypted at rest (vault); encrypted backups |
| MITM between the controller and nodes | SSH with pinned host keys; quarantine on key change |
| Hostile data from clients or nodes (hostnames, SSIDs, DHCP options, log lines) | Treated as untrusted: strict parsing, size limits, no `innerHTML` (Solid escapes by default), a strict CSP |
| Command injection through config values | Agent ops take argv arrays and JSON, never shell strings; the UCI serializer validates and escapes values |
| A compromised node feeding bogus telemetry | Everything is validated and bounded; a node can't affect other nodes' config; anomalies raise alerts |
| A rogue device getting adopted | Adoption needs explicit admin approval (ZTP has no auto-trust); host-key fingerprints are shown |
| Supply-chain compromise | Locked dependencies, cargo-deny, Dependabot, SBOM and provenance attestations, signed images |

## Controller authentication & sessions

- **Users:** the first run creates the admin; there are no defaults. Local users have roles admin or viewer (viewer arrives in M9).
- **Passwords:** Argon2id, with memory-hard parameters tuned for a Raspberry Pi, and a minimum length.
- **Sessions:**
  - random 256-bit IDs in HttpOnly + Secure + SameSite=Strict cookies
  - idle and absolute timeouts; revocation
  - CSRF defense: SameSite, an Origin check, and a custom header on mutating requests
- **API tokens:** scoped and revocable, for scripts.
- **2FA:** TOTP (M9). Passkeys (WebAuthn) once the controller is served with a trusted certificate.
- **Audit log:** every login, change, apply, terminal session and secret view.

## HTTP response headers

Every response carries these headers (`crates/server/src/http/security.rs`); a handler that needs a different value sets it first.

- `Content-Security-Policy: default-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'`. There are no inline scripts or styles: the UI sets dynamic geometry through the CSSOM, which the policy allows.
- `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Resource-Policy: same-origin`.
- API responses are `Cache-Control: no-store`, and API errors are RFC 9457 problem+json.

## TLS

The controller is its own small CA (D-039, `crates/server/src/tls/`):

- **The local CA:** ECDSA P-256, 10 years, `pathlen:0`, created on the first run and kept in the vault (`tls/ca`). Its critical name constraints permit only `localhost`, `lan`, `home.arpa`, `local`, `internal`, the loopback, private (RFC 1918, RFC 4193), shared (100.64.0.0/10) and link-local ranges, plus the names the first run's leaf needed beyond them (the host name, `--tls-names`). So a device that trusts it cannot be fooled about any other site, even by someone who stole the CA's key. Its key is read from the vault only to sign.
- **The leaf:** ECDSA P-256, 397 days (Apple's limit is 398), server authentication only, served with the CA as its chain. It covers `localhost`, `uniwrt.lan`, the host name and `<host>.lan` (not Docker's 12-digit container IDs), loopback, the HTTPS listen address, the private addresses the host routes from, and `--tls-names` (`UNIWRT_TLS_NAMES`). A daily check reissues it 30 days before expiry, or when its names or its CA change, and swaps it in without a restart. Names the CA may not sign are left out, with a warning: configure them before the first run.
- **Trust:** `GET /api/v1/tls/ca.crt` downloads the CA (PEM, `application/x-x509-ca-cert`), and `GET /api/v1/tls` shows its SHA-256 fingerprint, which the log also prints at startup, to compare on the device. Trusting it once removes the warnings for good.
- **rustls** with the ring provider: TLS 1.3 and 1.2 with rustls' safe defaults, ALPN `h2` and `http/1.1`. Handshakes run concurrently, at most 128 at once and 10 s each.
- **Plain HTTP** (8080) redirects to HTTPS with a 307 (not cached, unlike 308). The exceptions carry only public data or come from a TLS-terminating proxy: the health check (Docker probes it), requests from `--trusted-proxies` (`UNIWRT_TRUSTED_PROXIES`, networks; they get the whole application), and later the ZTP hello endpoint.
- **No HSTS** while the certificate comes from the local CA: on a device that does not trust it, HSTS would make the warning impossible to click through. Custom certificate upload and ACME DNS-01 are in the backlog; HSTS comes with them.

## Secrets vault

- Each secret is sealed with XChaCha20-Poly1305 under an HKDF-SHA256 subkey of the master key (`uniwrt vault v1`), with a random 192-bit nonce and the secret's name as associated data, so a sealed value cannot be moved to another name (`crates/store/src/vault/`).
- **The master key** is 32 random bytes, kept as 64 hexadecimal digits: in `UNIWRT_MASTER_KEY`, or in a file, by default `/data/master.key` (mode 0600, created on the first run; a warning if others can read it). `--master-key-file` (`UNIWRT_MASTER_KEY_FILE`) puts the file elsewhere, such as a Docker secret, so that a copy of the data volume alone does not open the vault. Back the key up apart from the database: without it the secrets are lost.
- **Opening** the vault tries the key on every sealed row and refuses to start if any does not open, so a wrong key fails at once. A missing key file is replaced by a new key only while nothing is sealed. Opening also reseals the rows that the M1.2 stub stored unencrypted (`alg = 'plain'`, D-032). Demo mode's key exists only in memory.
- The first secrets are the controller's SSH key (`controller/ssh_key`), the local CA (`tls/ca`) and the HTTPS certificate (`tls/leaf`).
- It holds: the controller SSH private key, node root passwords, Wi-Fi passphrases, mesh and FT keys, PPPoE credentials, WireGuard keys and notifier tokens.
- Config revisions store only references. Viewing a secret is admin-only and audited.

## Node hardening (opt-in per node; recommended in managed mode)

- a random root password (in the vault) and key-only SSH
- dropbear bound to the management network
- WAN-side SSH and LuCI blocked (the OpenWrt default)
- LuCI optional; remove it on small devices
- a controller key rotation job: add the new key → verify → remove the old one

## Container

Runs as UID/GID 65532 and works with a read-only root filesystem (it writes only to `/data`). The image contains no shell or tools and needs no extra capabilities unless it binds ports below 1024.

## Supply chain & releases

- `Cargo.lock` and `bun.lock` are committed.
- npm dependencies are pinned to exact versions, and bun resolves only versions published at least 7 days ago, transitive ones included (`web/bunfig.toml`, D-027).
- cargo-deny (advisories, licenses, bans, sources) runs in CI.
- Dependabot covers cargo, the web packages (bun), GitHub Actions and the Dockerfile's base image, proposing only releases at least 7 days old; actions are pinned by commit SHA.
- Images ship with SBOM and provenance attestations and cosign keyless signatures; binaries ship with SHA256SUMS and build provenance attestations, and embed their dependency list (cargo-auditable).
