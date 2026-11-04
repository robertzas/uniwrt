//! UCI, OpenWrt's configuration format (docs/openwrt/REFERENCE.md, UCI syntax): the AST of a
//! package, a parser and a serializer that agree with libuci, and the semantic diff that the
//! review screen, transactions and drift detection read (features/lifecycle.md).
//!
//! - **API:** [`Package`] is one `/etc/config` file: [`Section`]s, each a type, an optional
//!   name and its [`Entry`]s, each an option or a list ([`Value`]). [`parse`] reads a file as
//!   `uci` loads it and [`parse_export`] what `uci export` prints; a [`ParseError`] names the
//!   line and byte `uci` names, in its words. [`Package::to_file`] writes the file as
//!   `uci commit` does, [`Package::to_export`] as `uci export` prints it, and
//!   [`Package::validate`] says whether the package reads back as itself. [`diff`] compares two
//!   versions ([`Diff`]: sections added, removed, changed or moved, at their [`SectionId`]s,
//!   with each [`OptionDiff`]); its `Display` is `uci show`-style lines. [`apply`] applies a
//!   diff to the package it came from. [`is_name`], [`is_package`] and [`is_type`] are libuci's
//!   identifier rules.
//! - **Invariants:** pure: no IO, no clock. Parsing is libuci's strict mode, ported with its
//!   in-place quirks and checked against `uci` from OpenWrt 24.10 and 25.12
//!   (`fixtures/`, `scripts/uci-conformance.sh`). Comments are dropped, as libuci drops them.
//!   For a package that validates, `parse(name, &p.to_file()) == Ok(p)`. Text is UTF-8: the
//!   caller decodes a node's files and refuses what is not. The diff's semantics are in
//!   [`diff`](mod@diff): order counts only among the sections of one type; `diff(a, b)` is
//!   empty exactly when `a` and `b` mean the same, and `apply(a, &diff(a, b))` means `b`.
//! - **Dependencies:** serde and thiserror; ts-rs with the `ts` feature.
//! - **Tests:** unit tests next to each part; `src/tests/` holds the conformance fixtures (real
//!   configs from both releases and cases for each rule, with `uci`'s output) and the property
//!   tests (`cargo nextest run -p uniwrt-uci`).

mod ast;
pub mod diff;
mod name;
mod parse;
#[cfg(test)]
mod tests;
mod write;

pub use self::{
    ast::{Entry, Invalid, Package, Problem, Section, Value},
    diff::{ApplyError, Diff, OptionDiff, SectionDiff, SectionId, apply, diff},
    name::{is_name, is_package, is_type},
    parse::{ParseError, Reason, parse, parse_export},
};
