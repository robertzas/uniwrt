//! Interactive shells on a pseudo-terminal (D-141): a [`Terminal`] is an [`ExecStream`] (a PTY
//! merges stderr into its output, so everything arrives as stdout) with a window size the
//! producer follows. Dropping it closes the channel, which hangs the shell up.

use bytes::Bytes;
use tokio::sync::watch;

use super::{ExecStream, Output, OutputSink, StdinSource, pipe};
use crate::Result;

/// The terminal type the shell is told (`$TERM`): what the browser's xterm.js emulates.
pub const TERM: &str = "xterm-256color";

/// A terminal's size in character cells, each within `1..=`[`PtySize::MAX`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    pub cols: u16,
    pub rows: u16,
}

impl PtySize {
    /// A terminal's size until it is told another.
    pub const DEFAULT: Self = Self { cols: 80, rows: 24 };
    /// The most columns, and rows.
    pub const MAX: u16 = 1000;

    /// `cols` × `rows`, unless either is out of bounds.
    pub const fn new(cols: u16, rows: u16) -> Option<Self> {
        if cols == 0 || rows == 0 || cols > Self::MAX || rows > Self::MAX {
            None
        } else {
            Some(Self { cols, rows })
        }
    }
}

/// A shell on a pseudo-terminal: write keystrokes, read what it shows, resize it.
#[derive(Debug)]
pub struct Terminal {
    stream: ExecStream,
    size: watch::Sender<PtySize>,
}

impl Terminal {
    /// Sends keystrokes (or a paste).
    pub async fn write(&self, data: impl Into<Bytes>) -> Result<()> {
        self.stream.write(data).await
    }

    /// The next output, `None` once the shell has ended. An `Err` ends it too.
    pub async fn next(&mut self) -> Option<Result<Output>> {
        self.stream.next().await
    }

    /// Tells the shell its window has a new size (nothing when it is the same).
    pub fn resize(&self, size: PtySize) {
        self.size.send_if_modified(|current| {
            let changed = *current != size;
            *current = size;
            changed
        });
    }

    pub fn size(&self) -> PtySize {
        *self.size.borrow()
    }
}

/// The producer's end of a [`Terminal`].
#[derive(Debug)]
pub struct TerminalSource {
    pub stdin: StdinSource,
    pub output: OutputSink,
    /// The window's size: it changes on a resize, and its `changed` fails once the
    /// [`Terminal`] is dropped (close the shell then).
    pub size: watch::Receiver<PtySize>,
}

/// A new terminal of `size` and its producer's end; see [`pipe`].
pub fn terminal_pipe(
    what: impl Into<std::sync::Arc<str>>,
    budget: usize,
    size: PtySize,
) -> (Terminal, TerminalSource) {
    let (stream, source) = pipe(what, budget);
    let (size, watched) = watch::channel(size);
    (
        Terminal { stream, size },
        TerminalSource {
            stdin: source.stdin,
            output: source.output,
            size: watched,
        },
    )
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::{PtySize, terminal_pipe};
    use crate::exec::Output;

    #[test]
    fn sizes_are_bounded() {
        assert_eq!(PtySize::new(80, 24), Some(PtySize::DEFAULT));
        assert_eq!(PtySize::new(0, 24), None);
        assert_eq!(PtySize::new(80, 0), None);
        assert_eq!(PtySize::new(1001, 24), None);
        assert!(PtySize::new(PtySize::MAX, PtySize::MAX).is_some());
    }

    #[tokio::test]
    async fn resizes_reach_the_producer_once_and_dropping_the_terminal_ends_them() {
        let (mut terminal, mut source) = terminal_pipe("a shell", 1024, PtySize::DEFAULT);
        terminal.resize(PtySize::DEFAULT);
        assert!(!source.size.has_changed().unwrap());
        let wide = PtySize {
            cols: 120,
            rows: 40,
        };
        terminal.resize(wide);
        source.size.changed().await.unwrap();
        assert_eq!(*source.size.borrow_and_update(), wide);
        assert_eq!(terminal.size(), wide);

        terminal.write(Bytes::from_static(b"ls\r")).await.unwrap();
        assert_eq!(
            source.stdin.next().await.unwrap(),
            Bytes::from_static(b"ls\r")
        );
        assert!(
            source
                .output
                .push(Output::Stdout(Bytes::from_static(b"ls\r\n")))
        );
        assert!(matches!(terminal.next().await, Some(Ok(Output::Stdout(_)))));

        drop(terminal);
        assert!(source.size.changed().await.is_err());
        assert_eq!(source.stdin.next().await, None);
    }
}
