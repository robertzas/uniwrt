//! The parser: a port of libuci's (file.c, strict mode, as `uci` and rpcd run it), so text
//! reads here as on a node, quirks included. libuci parses each line in place in a buffer,
//! and some outcomes depend on that: a `;` glued to an unquoted word ends the line, while after
//! a quoted part it separates statements. The port keeps the buffer, positions and line count,
//! so errors name the line and byte `uci` names.
//!
//! What it reads: `config <type> [<name>]`, `option <name> [<value>]`, `list <name> [<value>]`
//! and `package <name>` (or `c`, `o`, `l`, `p`); words quoted with `'…'` (literal, may span
//! lines) or `"…"` (`\` takes the next character literally), or bare (`\` likewise; `\` at the
//! end of a line continues it); parts of a word concatenate; `#` outside quotes starts a
//! comment, which is dropped; `;` separates statements. An `option` without a value is skipped,
//! a `list` without one adds an empty item, a `list` after an `option` of the same name makes
//! a list of both, and an `option` after a list replaces it in its place. A named section that
//! comes again is the same section (another type is an error).

mod port;

use thiserror::Error;

use self::port::Parser;
use crate::ast::Package;

/// Parses a file of `/etc/config`, as `uci` loads a package. `package` statements are checked
/// and ignored.
pub fn parse(package: &str, text: &str) -> Result<Package, ParseError> {
    reject_nul(text)?;
    let mut parsed = Parser::new(text, true, Some(package.to_owned())).run()?;
    Ok(parsed.pop().unwrap_or_else(|| Package::new(package)))
}

/// Parses what `uci export` prints: packages, each from its `package` statement on.
pub fn parse_export(text: &str) -> Result<Vec<Package>, ParseError> {
    reject_nul(text)?;
    Parser::new(text, false, None).run()
}

/// Where and why the text does not parse; [`std::fmt::Display`] reads as `uci`'s message.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{reason} at line {line}, byte {byte}")]
pub struct ParseError {
    pub line: usize,
    pub byte: usize,
    pub reason: Reason,
}

/// libuci's reasons, in its words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum Reason {
    #[error("invalid command")]
    InvalidCommand,
    #[error("too many arguments")]
    TooManyArguments,
    #[error("insufficient arguments")]
    InsufficientArguments,
    #[error("invalid character in name field")]
    InvalidName,
    #[error("invalid character in type field")]
    InvalidType,
    #[error("EOF with unterminated '")]
    UnterminatedSingle,
    #[error("EOF with unterminated \"")]
    UnterminatedDouble,
    #[error("option/list command found before the first section")]
    NoSection,
    #[error("section of different type overwrites prior section with same name")]
    TypeConflict,
    #[error("package without name")]
    PackageWithoutName,
    #[error("config without name")]
    ConfigWithoutName,
    #[error("option without name")]
    OptionWithoutName,
    #[error("attempting to import a file without a package name")]
    NoPackageName,
    #[error("duplicate package")]
    DuplicatePackage,
    /// Not libuci's: it reads a NUL as the end of the line and loses the rest.
    #[error("NUL byte")]
    Nul,
    /// Not libuci's: the name given for a file.
    #[error("invalid package name")]
    InvalidPackage,
}

/// libuci would read a NUL as the end of its line and lose the rest of it.
fn reject_nul(text: &str) -> Result<(), ParseError> {
    let Some(at) = text.find('\0') else {
        return Ok(());
    };
    let before = &text[..at];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    Err(ParseError {
        line: before.matches('\n').count() + 1,
        byte: at - line_start,
        reason: Reason::Nul,
    })
}

#[cfg(test)]
mod tests {
    use super::{Reason, parse, parse_export};

    #[test]
    fn export_streams() {
        let text = "package a\n\nconfig t 'n'\n\toption x '1'\n\npackage b\n\nconfig u\n\n";
        let packages = parse_export(text).unwrap();
        let names: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(packages[0].section("n").unwrap().option("x"), Some("1"));
        assert_eq!(packages[1].sections_of("u").count(), 1);
        let reason = |text| parse_export(text).unwrap_err().reason;
        assert_eq!(reason("config t\n"), Reason::NoPackageName);
        assert_eq!(reason("package a\npackage a\n"), Reason::DuplicatePackage);
        assert_eq!(parse_export("").unwrap(), []);
    }

    #[test]
    fn refuses_what_uci_would_misread() {
        let error = parse("p", "config a\n\toption x 'a\0b'\n").unwrap_err();
        assert_eq!((error.reason, error.line, error.byte), (Reason::Nul, 2, 12));
        assert_eq!(parse("a.b", "").unwrap_err().reason, Reason::InvalidPackage);
        assert_eq!(parse("p", "").unwrap().sections, []);
    }
}
