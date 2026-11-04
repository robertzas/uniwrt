//! A diff as text, in `uci show`'s terms: a line per section and per option, `+` for what came,
//! `-` for what went, and a context line, starting with a space, for a changed section.
//!
//! ```text
//!  network.lan=interface
//! -network.lan.ipaddr='192.168.1.1/24'
//! +network.lan.ipaddr='10.0.0.1/24'
//! +firewall.@rule[9]=rule
//! +firewall.@rule[9].name='Allow-SSH'
//!  firewall.@rule[2]=rule (moved from @rule[4])
//! ```

use std::fmt;

use super::{Diff, SectionId};
use crate::{ast::Value, write::quote};

impl fmt::Display for Diff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let package = &self.package;
        for change in &self.sections {
            let Some(at) = change.after.as_ref().or(change.before.as_ref()) else {
                continue;
            };
            match (&change.before, &change.after) {
                (Some(before), Some(after)) => {
                    let path = after;
                    if before.kind == after.kind {
                        write!(f, " {package}.{path}={}", after.kind)?;
                    } else {
                        writeln!(f, "-{package}.{path}={}", before.kind)?;
                        write!(f, "+{package}.{path}={}", after.kind)?;
                    }
                    if change.moved {
                        write!(f, " (moved from {before})")?;
                    }
                    writeln!(f)?;
                }
                (before, _) => {
                    let sign = if before.is_some() { '-' } else { '+' };
                    writeln!(f, "{sign}{package}.{}={}", at, at.kind)?;
                }
            }
            for option in &change.options {
                let path = format!("{package}.{}.{}", at, option.name);
                if let Some(value) = &option.before {
                    writeln!(f, "-{path}={}", values(value))?;
                }
                if let Some(value) = &option.after {
                    writeln!(f, "+{path}={}", values(value))?;
                }
            }
        }
        Ok(())
    }
}

/// `name`, or `@type[index]` for an anonymous section.
impl fmt::Display for SectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.name {
            Some(name) => f.write_str(name),
            None => write!(f, "@{}[{}]", self.kind, self.index),
        }
    }
}

/// A value as `uci show` prints it: each item quoted, a list's separated by spaces.
fn values(value: &Value) -> String {
    let mut out = String::new();
    for (k, item) in value.items().iter().enumerate() {
        if k > 0 {
            out.push(' ');
        }
        quote(&mut out, item);
    }
    out
}
