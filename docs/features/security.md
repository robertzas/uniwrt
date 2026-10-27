# Security

Read this when working on authentication, sessions, TLS, secrets, node hardening, the container, or supply chain and releases.
Milestones: M1.6 (auth, TLS, vault), M3.1 (vault for config secrets), M9.3 (review, fuzzing).

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

- First run generates a self-signed ECDSA certificate (SANs: hostname, `uniwrt.lan`, IPs). It can be downloaded so devices can trust it.
- Custom certificate upload is supported; ACME DNS-01 is in the backlog. HSTS is enabled once a trusted certificate is configured.
- Plain HTTP only redirects to HTTPS. The one exception is the ZTP hello endpoint, which carries only public data.

## Secrets vault

- Each secret is encrypted with XChaCha20-Poly1305. The master key is `/data/master.key` (mode 0600) or `UNIWRT_MASTER_KEY`.
- Until M1.6, the vault is a stub (D-032): the `vault` table stores values unencrypted, marked `alg = 'plain'`, and M1.6 reseals those rows when it opens. The first secret is the controller's SSH key (`controller/ssh_key`), generated on the first run.
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
