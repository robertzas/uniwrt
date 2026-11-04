//! The serializer: text as `uci commit` writes it (libuci's `uci_export_package`), so a file
//! the controller renders is byte for byte the file `uci` would write after a commit.
//!
//! Every section starts with a blank line, `config <type>` and its name in single quotes;
//! options and lists follow indented by a tab, each value in single quotes with `'` written as
//! `'\''`; the file ends with a blank line. Comments are not kept: libuci keeps none either.
//! One departure: a type holding `'`, `"`, `#`, `;` or `\` is quoted, where libuci writes it
//! bare and then cannot read it back.

use std::fmt::Write as _;

use crate::ast::{Package, Value};

impl Package {
    /// The file, as `uci commit` writes it.
    pub fn to_file(&self) -> String {
        let mut out = String::new();
        for section in &self.sections {
            out.push_str("\nconfig ");
            if section.kind.contains(['\'', '"', '#', ';', '\\']) {
                quote(&mut out, &section.kind);
            } else {
                out.push_str(&section.kind);
            }
            if let Some(name) = &section.name {
                out.push(' ');
                quote(&mut out, name);
            }
            out.push('\n');
            for entry in &section.options {
                let keyword = match entry.value {
                    Value::Single(_) => "option",
                    Value::List(_) => "list",
                };
                for item in entry.value.items() {
                    let _ = write!(out, "\t{keyword} {} ", entry.name);
                    quote(&mut out, item);
                    out.push('\n');
                }
            }
        }
        out.push('\n');
        out
    }

    /// The package as `uci export` prints it: `package <name>`, then the file.
    pub fn to_export(&self) -> String {
        format!("package {}\n{}", self.name, self.to_file())
    }
}

/// A value in single quotes, as libuci's `uci_escape` writes it.
pub(crate) fn quote(out: &mut String, value: &str) {
    out.push('\'');
    out.push_str(&value.replace('\'', "'\\''"));
    out.push('\'');
}
