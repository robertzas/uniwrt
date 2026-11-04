//! The port of libuci's parser: its buffer, positions and steps, function by function.

use std::{iter::Peekable, str::SplitInclusive};

use super::{ParseError, Reason};
use crate::{
    ast::{Package, Section, Value},
    name::{is_package, is_type},
};

/// C's `isspace` in the C locale.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// libuci's `uci_validate_str`.
fn valid(s: &[u8], name: bool, package: bool) -> bool {
    !s.is_empty()
        && s.iter().all(|&c| {
            c.is_ascii_alphanumeric()
                || c == b'_'
                || (c == b'-' && package)
                || (!name && (33..=126).contains(&c))
        })
}

pub(super) struct Parser<'a> {
    lines: Peekable<SplitInclusive<'a, char>>,
    /// The line being parsed, NUL-terminated in places as libuci's buffer is.
    buf: Vec<u8>,
    pos: usize,
    line: usize,
    /// One file: `package` statements are ignored.
    single: bool,
    /// The package the next section opens (`pctx->name`).
    name: Option<String>,
    package: Option<Package>,
    /// The current section's index in `package`.
    section: Option<usize>,
    done: Vec<Package>,
}

type Step<T = ()> = Result<T, Reason>;

impl<'a> Parser<'a> {
    pub(super) fn new(text: &'a str, single: bool, name: Option<String>) -> Self {
        Self {
            lines: text.split_inclusive('\n').peekable(),
            buf: Vec::new(),
            pos: 0,
            line: 0,
            single,
            name,
            package: None,
            section: None,
            done: Vec::new(),
        }
    }

    pub(super) fn run(mut self) -> Result<Vec<Package>, ParseError> {
        let fail = |p: &Self, reason| ParseError {
            line: p.line,
            byte: p.pos,
            reason,
        };
        if self.name.as_deref().is_some_and(|name| !is_package(name)) {
            return Err(fail(&self, Reason::InvalidPackage));
        }
        while self.lines.peek().is_some() {
            self.pos = 0;
            self.getln(0);
            if self.byte(0) != 0 {
                self.parse_line().map_err(|reason| fail(&self, reason))?;
            }
        }
        if self.package.is_none() && self.name.is_some() {
            self.switch().map_err(|reason| fail(&self, reason))?;
        }
        self.name = None;
        self.switch().map_err(|reason| fail(&self, reason))?;
        Ok(self.done)
    }

    // The buffer, as C sees it: past the end reads as NUL.

    fn byte(&self, at: usize) -> u8 {
        self.buf.get(at).copied().unwrap_or(0)
    }

    fn cur(&self) -> u8 {
        self.byte(self.pos)
    }

    fn put(&mut self, at: usize, b: u8) {
        if let Some(slot) = self.buf.get_mut(at) {
            *slot = b;
        }
    }

    /// The NUL-terminated string at `at`.
    fn str_at(&self, at: usize) -> &[u8] {
        let rest = self.buf.get(at..).unwrap_or_default();
        &rest[..rest.iter().position(|&b| b == 0).unwrap_or(rest.len())]
    }

    fn string_at(&self, at: usize) -> String {
        // Whole UTF-8 sequences only move or go: the bytes dropped are ASCII.
        String::from_utf8_lossy(self.str_at(at)).into_owned()
    }

    /// `uci_getln`: reads the next line into the buffer at `offset`.
    fn getln(&mut self, offset: usize) {
        self.buf.truncate(offset);
        if let Some(line) = self.lines.next() {
            self.buf.extend_from_slice(line.as_bytes());
            if line.ends_with('\n') {
                self.line += 1;
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while self.cur() != 0 && is_space(self.cur()) {
            self.pos += 1;
        }
    }

    fn increase_pos(&mut self, add: usize, reason: Reason) -> Step {
        if self.pos + add > self.buf.len() {
            return Err(reason);
        }
        self.pos += add;
        Ok(())
    }

    /// Skips the command word that `strtok` cut off at the position.
    fn skip_command(&mut self, reason: Reason) -> Step {
        let len = self.str_at(self.pos).len();
        self.increase_pos(len + 1, reason)
    }

    /// `uci_parse_line`: the statements of one line.
    fn parse_line(&mut self) -> Step {
        self.skip_whitespace();
        loop {
            // strtok(cur, " \t"): the next word, cut off with a NUL; the position stays.
            let mut start = self.pos;
            while matches!(self.byte(start), b' ' | b'\t') {
                start += 1;
            }
            if self.byte(start) == 0 {
                return Ok(());
            }
            let mut end = start;
            while !matches!(self.byte(end), 0 | b' ' | b'\t') {
                end += 1;
            }
            self.put(end, 0);
            let command = self.buf[start..end].to_vec();
            match command.as_slice() {
                [b'#', ..] => return Ok(()),
                b"p" | b"package" => self.parse_package()?,
                b"c" | b"config" => self.parse_config()?,
                b"o" | b"option" => self.parse_option(false)?,
                b"l" | b"list" => self.parse_option(true)?,
                _ => return Err(Reason::InvalidCommand),
            }
        }
    }

    fn parse_package(&mut self) -> Step {
        self.skip_command(Reason::PackageWithoutName)?;
        let at = self.next_arg(true, true, true)?;
        self.assert_eol()?;
        if !self.single {
            self.name = Some(self.string_at(at));
            self.switch()?;
        }
        Ok(())
    }

    /// `uci_switch_config`: closes the package and opens the one `name` names.
    fn switch(&mut self) -> Step {
        if let Some(package) = self.package.take() {
            self.done.push(package);
            self.section = None;
        }
        let Some(name) = &self.name else {
            return Ok(());
        };
        if self.done.iter().any(|p| &p.name == name) {
            return Err(Reason::DuplicatePackage);
        }
        self.package = Some(Package::new(name.clone()));
        Ok(())
    }

    fn parse_config(&mut self) -> Step {
        if self.package.is_none() {
            if self.name.is_none() {
                return Err(Reason::NoPackageName);
            }
            self.switch()?;
        }
        self.skip_command(Reason::ConfigWithoutName)?;
        let kind_at = self.next_arg(true, false, false)?;
        let kind = self.string_at(kind_at);
        if !is_type(&kind) {
            return Err(Reason::InvalidType);
        }
        let name_at = self.next_arg(false, true, false)?;
        self.assert_eol()?;
        let name = self.string_at(name_at);
        let Some(package) = self.package.as_mut() else {
            return Err(Reason::NoPackageName);
        };
        let sections = &mut package.sections;
        let found = (!name.is_empty())
            .then(|| {
                sections
                    .iter()
                    .position(|s| s.name.as_deref() == Some(&*name))
            })
            .flatten();
        self.section = Some(match found {
            Some(index) if sections[index].kind != kind => return Err(Reason::TypeConflict),
            Some(index) => index,
            None => {
                sections.push(if name.is_empty() {
                    Section::anonymous(kind)
                } else {
                    Section::named(kind, name)
                });
                sections.len() - 1
            }
        });
        Ok(())
    }

    fn parse_option(&mut self, list: bool) -> Step {
        let Some(index) = self.section else {
            return Err(Reason::NoSection);
        };
        self.skip_command(Reason::OptionWithoutName)?;
        let name_at = self.next_arg(true, true, false)?;
        let value_at = self.next_arg(false, false, false)?;
        self.assert_eol()?;
        let (name, value) = (self.string_at(name_at), self.string_at(value_at));
        let Some(section) = self
            .package
            .as_mut()
            .and_then(|p| p.sections.get_mut(index))
        else {
            return Err(Reason::NoSection);
        };
        if list {
            section.add_list(&name, value);
        } else if !value.is_empty() {
            section.set(&name, Value::Single(value));
        }
        Ok(())
    }

    /// `assert_eol`: nothing but a comment or a `;` may follow.
    fn assert_eol(&mut self) -> Step {
        self.skip_whitespace();
        let at = self.next_arg(false, false, false)?;
        if self.byte(at) != 0 {
            return Err(Reason::TooManyArguments);
        }
        Ok(())
    }

    /// `next_arg`: the next word, unquoted in place; returns where it starts.
    fn next_arg(&mut self, required: bool, name: bool, package: bool) -> Step<usize> {
        self.skip_whitespace();
        let at = self.pos;
        if self.cur() == b';' {
            self.put(self.pos, 0);
            self.pos += 1;
        } else {
            self.parse_str(at)?;
        }
        if self.byte(at) == 0 {
            return if required {
                Err(Reason::InsufficientArguments)
            } else {
                Ok(at)
            };
        }
        if name && !valid(self.str_at(at), name, package) {
            return Err(Reason::InvalidName);
        }
        Ok(at)
    }

    /// `parse_str`: one word, its parts unquoted and joined at `target`.
    fn parse_str(&mut self, mut target: usize) -> Step {
        let mut next = true;
        loop {
            match self.cur() {
                b'\'' => self.single_quote(&mut target)?,
                b'"' => self.double_quote(&mut target)?,
                b'#' => {
                    self.put(self.pos, 0);
                    break;
                }
                0 => break,
                b';' => {
                    next = false;
                    break;
                }
                b'\\' => {
                    if self.backslash() {
                        self.addc(&mut target);
                    }
                }
                _ => self.addc(&mut target),
            }
            if self.cur() == 0 || is_space(self.cur()) {
                break;
            }
        }
        if self.cur() != 0 && next {
            self.pos += 1;
        }
        self.put(target, 0);
        Ok(())
    }

    fn addc(&mut self, target: &mut usize) {
        let b = self.cur();
        self.put(*target, b);
        *target += 1;
        self.pos += 1;
    }

    /// `parse_backslash`: false when it continued the line, true when the next character is
    /// to be taken.
    fn backslash(&mut self) -> bool {
        self.pos += 1;
        let c = self.cur();
        let crlf = c == b'\r' && self.byte(self.pos + 1) == b'\n' && self.byte(self.pos + 2) == 0;
        if c == 0 || c == b'\n' || crlf {
            self.getln(self.pos);
            return false;
        }
        true
    }

    fn single_quote(&mut self, target: &mut usize) -> Step {
        self.pos += 1;
        loop {
            match self.cur() {
                b'\'' => {
                    self.pos += 1;
                    return Ok(());
                }
                0 => {
                    self.getln(self.pos);
                    if self.cur() == 0 {
                        return Err(Reason::UnterminatedSingle);
                    }
                }
                _ => self.addc(target),
            }
        }
    }

    fn double_quote(&mut self, target: &mut usize) -> Step {
        self.pos += 1;
        loop {
            match self.cur() {
                b'"' => {
                    self.pos += 1;
                    return Ok(());
                }
                0 => {
                    self.getln(self.pos);
                    if self.cur() == 0 {
                        return Err(Reason::UnterminatedDouble);
                    }
                }
                b'\\' => {
                    if self.backslash() {
                        self.addc(target);
                    }
                }
                _ => self.addc(target),
            }
        }
    }
}
