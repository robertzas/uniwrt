//! Splits a byte stream into lines, bounded in length so a broken peer cannot exhaust memory.

/// Accumulates chunks and yields complete lines without their `\n` (or `\r\n`).
#[derive(Debug)]
pub(crate) struct LineBuf {
    buf: Vec<u8>,
    /// Start of the first line not yet returned.
    start: usize,
    /// Where to resume looking for `\n`: everything before it is part of the current line.
    scan: usize,
    max: usize,
}

/// A line grew past the limit without ending.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TooLong;

impl LineBuf {
    pub(crate) fn new(max: usize) -> Self {
        Self {
            buf: Vec::new(),
            start: 0,
            scan: 0,
            max,
        }
    }

    pub(crate) fn push(&mut self, chunk: &[u8]) {
        if self.start > 0 {
            self.buf.drain(..self.start);
            self.scan -= self.start;
            self.start = 0;
        }
        self.buf.extend_from_slice(chunk);
    }

    /// The next complete line, if there is one.
    pub(crate) fn next_line(&mut self) -> Result<Option<&[u8]>, TooLong> {
        let Some(offset) = self.buf[self.scan..].iter().position(|&byte| byte == b'\n') else {
            self.scan = self.buf.len();
            if self.buf.len() - self.start > self.max {
                return Err(TooLong);
            }
            return Ok(None);
        };
        let end = self.scan + offset;
        let line = &self.buf[self.start..end];
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        self.start = end + 1;
        self.scan = self.start;
        if line.len() > self.max {
            return Err(TooLong);
        }
        Ok(Some(line))
    }
}

#[cfg(test)]
mod tests {
    use super::{LineBuf, TooLong};

    fn drain(lines: &mut LineBuf) -> Vec<String> {
        let mut out = Vec::new();
        while let Some(line) = lines.next_line().unwrap() {
            out.push(String::from_utf8(line.to_vec()).unwrap());
        }
        out
    }

    #[test]
    fn lines_split_across_and_within_chunks() {
        let mut lines = LineBuf::new(100);
        lines.push(b"{\"a\":");
        assert_eq!(drain(&mut lines), Vec::<String>::new());
        lines.push(b"1}\n{\"b\":2}\r\n\n{\"c\"");
        assert_eq!(drain(&mut lines), [r#"{"a":1}"#, r#"{"b":2}"#, ""]);
        lines.push(b":3}\n");
        assert_eq!(drain(&mut lines), [r#"{"c":3}"#]);
        assert_eq!(drain(&mut lines), Vec::<String>::new());
    }

    #[test]
    fn a_line_past_the_limit_is_refused_even_before_it_ends() {
        let mut lines = LineBuf::new(4);
        lines.push(b"1234\n12345\n");
        assert_eq!(lines.next_line().unwrap(), Some(&b"1234"[..]));
        assert_eq!(lines.next_line(), Err(TooLong));

        let mut lines = LineBuf::new(4);
        lines.push(b"123");
        assert_eq!(lines.next_line(), Ok(None));
        lines.push(b"45");
        assert_eq!(lines.next_line(), Err(TooLong));
    }

    #[test]
    fn byte_at_a_time_input_resumes_the_scan() {
        let mut lines = LineBuf::new(1 << 20);
        let text = "x".repeat(5_000) + "\n";
        let (body, newline) = text.as_bytes().split_at(5_000);
        for byte in body {
            lines.push(&[*byte]);
            assert_eq!(lines.next_line(), Ok(None));
            assert_eq!(lines.scan, lines.buf.len());
        }
        lines.push(newline);
        assert_eq!(lines.next_line().unwrap().map(<[u8]>::len), Some(5_000));
        assert_eq!(lines.next_line(), Ok(None));
    }
}
