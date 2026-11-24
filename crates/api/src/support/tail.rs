//! The controller's own log, its latest lines kept in memory (at most [`LINES`] lines and
//! [`BYTES`] bytes) for the support bundle: the server's logger writes every line here as well
//! as to stderr.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, PoisonError},
};

pub(crate) const LINES: usize = 2_000;
pub(crate) const BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Default)]
pub struct LogTail(Arc<Mutex<Ring>>);

#[derive(Debug, Default)]
struct Ring {
    lines: VecDeque<String>,
    bytes: usize,
    /// A line not ended yet.
    partial: String,
}

impl LogTail {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends what the logger wrote; whole lines are kept, the oldest dropped.
    pub fn write(&self, data: &[u8]) {
        let mut ring = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let text = String::from_utf8_lossy(data);
        for piece in text.split_inclusive('\n') {
            ring.partial.push_str(piece);
            if ring.partial.ends_with('\n') {
                let line = std::mem::take(&mut ring.partial);
                ring.bytes += line.len();
                ring.lines.push_back(line);
                while ring.lines.len() > LINES || ring.bytes > BYTES {
                    let Some(old) = ring.lines.pop_front() else {
                        break;
                    };
                    ring.bytes -= old.len();
                }
            }
        }
    }

    /// The kept lines, oldest first.
    pub fn text(&self) -> String {
        let ring = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        ring.lines.iter().map(String::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{LINES, LogTail};

    #[test]
    fn whole_lines_are_kept_and_the_oldest_dropped() {
        let tail = LogTail::new();
        tail.write(b"one\ntw");
        assert_eq!(tail.text(), "one\n");
        tail.write(b"o\n");
        assert_eq!(tail.text(), "one\ntwo\n");
        for i in 0..LINES {
            tail.write(format!("line {i}\n").as_bytes());
        }
        let text = tail.text();
        assert!(!text.contains("two"));
        assert!(text.starts_with("line 0\n"));
        assert_eq!(text.lines().count(), LINES);
    }
}
