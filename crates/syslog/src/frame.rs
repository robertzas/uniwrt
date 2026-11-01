//! Syslog over TCP (RFC 6587): each frame is octet-counted (`LEN SP MSG`) or ends with a line
//! feed or a NUL (non-transparent framing; logd ends its messages with `\n`, or `\0` with
//! `log_trailer_null`). A frame that starts with a digit is counted, any other is delimited.

use crate::parse::MAX_MESSAGE;

/// The longest frame read; the rest of a longer one is skipped.
const MAX_FRAME: usize = MAX_MESSAGE + 1024;

/// Splits a TCP stream into frames, whatever chunks it arrives in.
#[derive(Debug, Default)]
pub struct Framer {
    buf: Vec<u8>,
    /// Bytes of an oversized frame still to skip: a count, or until the delimiter.
    skip: Skip,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Skip {
    #[default]
    None,
    Bytes(usize),
    ToDelimiter,
}

impl Framer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds what was read, and returns the frames it completed, oversized ones cut to
    /// [`MAX_FRAME`].
    pub fn push(&mut self, mut data: &[u8]) -> Vec<Vec<u8>> {
        match self.skip {
            Skip::None => {}
            Skip::Bytes(n) => {
                let skipped = n.min(data.len());
                data = &data[skipped..];
                self.skip = if skipped == n {
                    Skip::None
                } else {
                    Skip::Bytes(n - skipped)
                };
            }
            Skip::ToDelimiter => match data.iter().position(|&b| b == b'\n' || b == 0) {
                Some(end) => {
                    data = &data[end + 1..];
                    self.skip = Skip::None;
                }
                None => data = &[],
            },
        }
        self.buf.extend_from_slice(data);
        let mut frames = Vec::new();
        let mut start = 0;
        while let Some((frame, next)) = self.next_frame(start) {
            if !frame.is_empty() {
                frames.push(frame);
            }
            start = next;
        }
        self.buf.drain(..start);
        frames
    }

    /// The frame starting at `start` and where the next starts, when it is complete; an
    /// oversized one is cut and the rest marked to skip.
    fn next_frame(&mut self, start: usize) -> Option<(Vec<u8>, usize)> {
        let rest = &self.buf[start..];
        // Delimiters between frames (`\r\n` endings, keep-alive newlines).
        let blank = rest
            .iter()
            .take_while(|&&b| matches!(b, b'\n' | b'\r' | 0))
            .count();
        if blank > 0 {
            return Some((Vec::new(), start + blank));
        }
        let first = *rest.first()?;
        if first.is_ascii_digit() {
            let space = rest.iter().take(8).position(|&b| b == b' ');
            let Some(space) = space else {
                // Not a count after all: up to 7 digits and a space, or it is delimited.
                return (rest.len() >= 8).then(|| self.delimited(start)).flatten();
            };
            let len: usize = std::str::from_utf8(&rest[..space]).ok()?.parse().ok()?;
            let body = &rest[space + 1..];
            if len > MAX_FRAME {
                let have = body.len().min(MAX_FRAME);
                if have < MAX_FRAME {
                    return None;
                }
                let frame = body[..MAX_FRAME].to_vec();
                let taken = body.len().min(len);
                if taken < len {
                    self.skip = Skip::Bytes(len - taken);
                }
                return Some((frame, start + space + 1 + taken));
            }
            if body.len() < len {
                return None;
            }
            return Some((body[..len].to_vec(), start + space + 1 + len));
        }
        self.delimited(start)
    }

    fn delimited(&mut self, start: usize) -> Option<(Vec<u8>, usize)> {
        let rest = &self.buf[start..];
        match rest.iter().position(|&b| b == b'\n' || b == 0) {
            Some(end) => Some((rest[..end.min(MAX_FRAME)].to_vec(), start + end + 1)),
            None if rest.len() > MAX_FRAME => {
                self.skip = Skip::ToDelimiter;
                Some((rest[..MAX_FRAME].to_vec(), start + rest.len()))
            }
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{Framer, MAX_FRAME};

    fn frames(chunks: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut framer = Framer::new();
        chunks.iter().flat_map(|chunk| framer.push(chunk)).collect()
    }

    #[test]
    fn delimited_and_counted_frames_split_across_chunks() {
        assert_eq!(
            frames(&[b"<13>a\n<13>b\0<1", b"3>c\r\n", b"\n"]),
            [b"<13>a".to_vec(), b"<13>b".to_vec(), b"<13>c\r".to_vec()]
        );
        assert_eq!(
            frames(&[b"5 <13>a7 <13>b\nc", b"d"]),
            [b"<13>a".to_vec(), b"<13>b\nc".to_vec()]
        );
        // Waiting for the rest.
        assert_eq!(frames(&[b"10 <13>"]), Vec::<Vec<u8>>::new());
        assert_eq!(frames(&[b"<13>no end yet"]), Vec::<Vec<u8>>::new());
    }

    #[test]
    fn oversized_frames_are_cut_and_their_rest_skipped() {
        let big = vec![b'x'; MAX_FRAME + 500];
        let mut delimited = big.clone();
        delimited.extend_from_slice(b"\n<13>next\n");
        let got = frames(&[&delimited[..MAX_FRAME + 10], &delimited[MAX_FRAME + 10..]]);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].len(), MAX_FRAME);
        assert_eq!(got[1], b"<13>next");
        let mut counted = format!("{} ", big.len()).into_bytes();
        counted.extend_from_slice(&big);
        counted.extend_from_slice(b"4 <1>x");
        let split = MAX_FRAME + 2;
        let got = frames(&[&counted[..split], &counted[split..]]);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].len(), MAX_FRAME);
        assert_eq!(got[1], b"<1>x");
    }

    proptest! {
        #[test]
        fn chunking_does_not_change_the_frames(
            messages in proptest::collection::vec("[a-z<>0-9 ]{1,40}", 1..20),
            counted in proptest::collection::vec(any::<bool>(), 20),
            cut in 1usize..50,
        ) {
            let mut stream = Vec::new();
            for (message, counted) in messages.iter().zip(&counted) {
                let message = format!("<13>{message}");
                if *counted {
                    stream.extend_from_slice(format!("{} {message}", message.len()).as_bytes());
                } else {
                    stream.extend_from_slice(message.as_bytes());
                    stream.push(b'\n');
                }
            }
            let whole = frames(&[&stream]);
            let chunks: Vec<&[u8]> = stream.chunks(cut).collect();
            prop_assert_eq!(frames(&chunks), whole.clone());
            prop_assert_eq!(whole.len(), messages.len());
        }
    }
}
