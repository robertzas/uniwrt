# Engineering conventions

These apply to humans and agents alike. Read only the section you need.

## Architecture rules

- Layering and allowed dependencies are defined in [ARCHITECTURE.md](../ARCHITECTURE.md#crates-and-layering). Pure (L0) crates use no tokio, do no IO, and never read the wall clock; inject a `Clock` instead.
- One crate, one responsibility, with a minimal public API.
- Every `lib.rs` starts with a `//!` **contract** covering purpose, public API summary, invariants, dependencies and how to test. Its first line is the one-line summary that `docs/MAP.md` picks up.
- Rust is the source of truth for cross-language types. DTOs derive `TS`, and `just gen` writes them to `web/src/api/gen/`. Never hand-edit generated files. `core` and `model` types derive it behind their `ts` feature (`#[cfg_attr(feature = "ts", derive(ts_rs::TS))]`); ts-rs's serde warnings are off, so a type with custom serde (`try_from`, a hand-written `Serialize`, a foreign field type) needs `ts(type = "…")` or `ts(as = "…")` (D-041).
- Node-side code lives in `node-agent/` (ucode plus POSIX sh). Arguments travel as JSON or argv arrays and are never interpolated into shell strings. The one exception is a checked integer formatted with `%d` where `popen` takes no argv (logread's line count, D-036).

## Rust

- Edition 2024 on the stable toolchain pinned in `rust-toolchain.toml`. `#![forbid(unsafe_code)]` unless an exception is justified in the crate contract.
- Errors: `thiserror` in libraries (typed, with context fields) and `anyhow` only in the binary. No `unwrap()` outside tests; `expect()` only with a stated invariant.
- Async: tokio. Never block in async code; SQLite runs on its own thread or actor. Loops must be cancellation-safe. `tracing` spans carry `node_id` and `job_id`.
- Lints: clippy with `-D warnings`, using workspace-level lint configuration.
- Keep files to about 400 lines or fewer and organize modules by concept. No `utils.rs` grab-bags.
- Tests live next to the code (`#[cfg(test)]`). Use snapshots via `insta` (review with `cargo insta review`) and property tests via `proptest` for parsers, IPAM and the planner.
- Performance: share large facts through `Arc` instead of cloning them, use bounded channels, and batch DB writes.
- Dependencies: crate versions, transitive ones included, must be at least 7 days old (D-029, D-031). After changing `Cargo.lock`, run `just crate-age`, which lists the crates it adds and fails inside the cooldown; pin a fresh one back with `cargo update -p <crate> --precise <version>`. The one exception is the lowest release that fixes a RustSec advisory: list it with the advisory ID in `scripts/crate-age.sh`, and drop the entry once it ages out (D-033).

## TypeScript / Solid

- Strict TypeScript and no `any` (enforced by Biome). Named exports. Components go in PascalCase files, one per file in `features/`; primitives go in `ui/`.
- Fetch data only through `lib/api` (the typed client) with solid-query. Live updates come through `lib/ws` subscriptions.
- Styling: Tailwind utilities plus tokens. No ad-hoc colors: Tailwind's default palette is removed, so only the tokens in `ui/tokens.css` exist. No inline styles except dynamic geometry set through the CSSOM (style props); the CSP blocks style attributes in markup.
- User-facing strings live in `lib/strings.ts`, so the UI is ready for translation. Icons come from lucide-solid; interactive primitives (dialogs, radio groups, menus) come from Kobalte.
- Features never import other features.
- Tests: Vitest + Testing Library for components that contain logic; Playwright for user flows (against demo mode). jsdom has no layout, so `src/test/setup.ts` stubs what Kobalte needs; components that use the router are tested inside a `MemoryRouter`.

## ucode / shell (node-agent)

- Target the ucode in OpenWrt 24.10 and later. Feature-detect modules (`require` inside `try`) and fall back to CLI tools.
- stdout carries exactly one JSON object per line and nothing else. Debug output goes to stderr.
- Write persistently only in the transaction and guard paths (`/etc/uniwrt`). Everything else goes under `/tmp/uniwrt`.
- Each collector is its own file exporting `collect(ctx)`, registered in `collect/index.uc`.

## Feature recipe (the cheap, repeatable path)

1. **Spec:** add or extend the section in `docs/features/<area>.md`. Record any decision in DECISIONS.md.
2. **Model:** types in `crates/model` (desired state and/or facts), plus validation, plus a migration if the schema changes.
3. **Render:** a module in `crates/render/src/<package>/` with golden tests (fixtures in `crates/render/tests/fixtures/`).
4. **Telemetry** (if needed): a collector in `node-agent/collect/<name>.uc`, a parser in `crates/node/src/collect/<name>.rs`, and a fixture JSON file.
5. **API:** routes in `crates/api/src/<area>` (a module per area, such as `auth/`, `nodes.rs`, `ws/`) with DTOs that derive `TS`, then run `just gen`. Live data goes out as a WS topic (`ws/topic.rs`, `ws/subs.rs`).
6. **UI:** `web/src/features/<area>/` (page plus components) built from `ui/` primitives, with its lazy-loaded route registered in `app/routes.tsx`.
7. **Docs:** tick the WP in ROADMAP and update STATUS. `docs/MAP.md` regenerates automatically.

From M1.9, `just new-feature <area>` scaffolds steps 2–6.

## Commits

- Format: [Conventional Commits](https://www.conventionalcommits.org/) as `type(scope): summary`.
- Types: feat, fix, perf, refactor, docs, test, build, ci, chore. Mark breaking changes with `!` or a `BREAKING CHANGE:` footer.
- Scope: a crate or area, for example `feat(render): …` or `fix(web/topology): …`.
- One logical change per commit. Commit once a significant change is complete and `just check` passes. The type drives the release version (see BUILD.md).
