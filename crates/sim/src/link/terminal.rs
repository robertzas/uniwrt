//! A simulated node's login shell (D-141): OpenWrt's banner and ash's prompt over a
//! [`Terminal`], with line editing (backspace, Ctrl-C, Ctrl-D, Ctrl-L; escape sequences such as
//! the arrows are ignored), answering a few commands as OpenWrt does (`echo`, `hostname`,
//! `uname`, `cat /etc/openwrt_release`, `pwd`, `whoami`, `clear`, `exit`) and saying that the
//! demo runs nothing else.

use bytes::Bytes;
use uniwrt_node::{
    ExitStatus, Output, PtySize, Terminal,
    exec::{STREAM_BUDGET, TerminalSource, terminal_pipe},
};

/// What the shell says about its node.
#[derive(Debug, Clone)]
pub(super) struct Host {
    pub(super) name: String,
    /// OpenWrt's version and revision, as `/etc/openwrt_release` has them.
    pub(super) version: &'static str,
    pub(super) revision: &'static str,
    pub(super) kernel: &'static str,
    /// The package architecture, such as `aarch64_cortex-a53`.
    pub(super) arch: &'static str,
    pub(super) target: &'static str,
}

const BANNER: &str = concat!(
    "  _______                     ________        __\r\n",
    " |       |.-----.-----.-----.|  |  |  |.----.|  |_\r\n",
    " |   -   ||  _  |  -__|     ||  |  |  ||   _||   _|\r\n",
    " |_______||   __|_____|__|__||________||__|  |____|\r\n",
    "          |__| W I R E L E S S   F R E E D O M\r\n",
);

/// Starts the shell.
pub(super) fn open(host: Host, size: PtySize) -> Terminal {
    let (terminal, source) =
        terminal_pipe(format!("a terminal on {}", host.name), STREAM_BUDGET, size);
    tokio::spawn(run(host, source));
    terminal
}

async fn run(host: Host, mut source: TerminalSource) {
    let mut shell = Shell::new(host);
    let hello = format!(
        "\r\n{BANNER} -----------------------------------------------------\r\n OpenWrt {}, {}\r\n -----------------------------------------------------\r\n{}",
        shell.host.version,
        shell.host.revision,
        shell.prompt()
    );
    if !say(&source, hello) {
        return;
    }
    loop {
        tokio::select! {
            data = source.stdin.next() => {
                let Some(data) = data else { return };
                let (shown, exit) = shell.feed(&data);
                if !shown.is_empty() && !say(&source, shown) {
                    return;
                }
                if let Some(code) = exit {
                    source.output.push(Output::Exit(ExitStatus::Code(code)));
                    return;
                }
            }
            // The terminal was dropped: the shell hangs up.
            changed = source.size.changed() => if changed.is_err() { return },
        }
    }
}

fn say(source: &TerminalSource, text: String) -> bool {
    source.output.push(Output::Stdout(Bytes::from(text)))
}

/// Where the line editor is within an escape sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Escape {
    None,
    /// After ESC.
    Start,
    /// Within a CSI or SS3 sequence, until its final byte.
    Body,
}

struct Shell {
    host: Host,
    line: String,
    escape: Escape,
}

impl Shell {
    fn new(host: Host) -> Self {
        Self {
            host,
            line: String::new(),
            escape: Escape::None,
        }
    }

    fn prompt(&self) -> String {
        format!("root@{}:~# ", self.host.name)
    }

    /// Takes keystrokes: what the terminal shows, and the exit code once the shell exits.
    fn feed(&mut self, data: &[u8]) -> (String, Option<u32>) {
        let mut shown = String::new();
        for c in String::from_utf8_lossy(data).chars() {
            match (self.escape, c) {
                (Escape::Start, '[' | 'O') => self.escape = Escape::Body,
                (Escape::Start, _) | (Escape::Body, '@'..='~') => self.escape = Escape::None,
                (Escape::None, '\u{1b}') => self.escape = Escape::Start,
                (Escape::None, '\r' | '\n') => {
                    shown.push_str("\r\n");
                    let line = std::mem::take(&mut self.line);
                    match self.run(&line) {
                        Ok(output) => shown.push_str(&output),
                        Err(code) => return (shown, Some(code)),
                    }
                    shown.push_str(&self.prompt());
                }
                (Escape::None, '\u{7f}' | '\u{8}') => {
                    if self.line.pop().is_some() {
                        shown.push_str("\u{8} \u{8}");
                    }
                }
                (Escape::None, '\u{3}') => {
                    self.line.clear();
                    shown.push_str("^C\r\n");
                    shown.push_str(&self.prompt());
                }
                (Escape::None, '\u{4}') if self.line.is_empty() => {
                    shown.push_str("\r\n");
                    return (shown, Some(0));
                }
                (Escape::None, '\u{c}') => {
                    shown.push_str("\u{1b}[H\u{1b}[2J");
                    shown.push_str(&self.prompt());
                    shown.push_str(&self.line);
                }
                (Escape::None, c) if !c.is_control() => {
                    self.line.push(c);
                    shown.push(c);
                }
                (Escape::Body | Escape::None, _) => {}
            }
        }
        (shown, None)
    }

    /// Runs `line`: its output, or the exit code when it is `exit`.
    fn run(&self, line: &str) -> Result<String, u32> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some((&command, args)) = words.split_first() else {
            return Ok(String::new());
        };
        let host = &self.host;
        let text = match (command, args) {
            ("exit", []) => return Err(0),
            ("exit", [code]) => return Err(code.parse().unwrap_or(2)),
            ("echo", args) => args.join(" "),
            ("hostname", []) | ("uname", ["-n"]) => host.name.clone(),
            ("pwd", []) => "/root".to_owned(),
            ("whoami", []) => "root".to_owned(),
            ("uname", []) => "Linux".to_owned(),
            ("uname", ["-r"]) => host.kernel.to_owned(),
            ("uname", ["-m"]) => machine(host.arch).to_owned(),
            ("uname", ["-a"]) => format!(
                "Linux {} {} #0 SMP {} GNU/Linux",
                host.name,
                host.kernel,
                machine(host.arch)
            ),
            ("cat", ["/etc/openwrt_release"]) => format!(
                "DISTRIB_ID='OpenWrt'\r\nDISTRIB_RELEASE='{}'\r\nDISTRIB_REVISION='{}'\r\nDISTRIB_TARGET='{}'\r\nDISTRIB_ARCH='{}'\r\nDISTRIB_DESCRIPTION='OpenWrt {} {}'",
                host.version, host.revision, host.target, host.arch, host.version, host.revision
            ),
            ("clear", []) => return Ok("\u{1b}[H\u{1b}[2J".to_owned()),
            _ => format!("-ash: {command}: the demo's devices run no other commands"),
        };
        Ok(format!("{text}\r\n"))
    }
}

/// `uname -m` of a package architecture.
fn machine(arch: &str) -> &str {
    match arch.split('_').next().unwrap_or(arch) {
        "arm" => "armv7l",
        "i386" => "i686",
        machine => machine,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bytes::Bytes;
    use uniwrt_node::{ExitStatus, Output, PtySize, Terminal};

    use super::{Host, Shell, open};

    fn host() -> Host {
        Host {
            name: "ap-garden".to_owned(),
            version: "24.10.4",
            revision: "r28959-29397011cc",
            kernel: "6.6.110",
            arch: "aarch64_cortex-a53",
            target: "mediatek/filogic",
        }
    }

    #[test]
    fn the_line_editor_echoes_erases_and_skips_escape_sequences() {
        let mut shell = Shell::new(host());
        assert_eq!(
            shell.feed(b"ecx\x7fho hi"),
            ("ecx\x08 \x08ho hi".to_owned(), None)
        );
        // The left arrow and Home are ignored.
        assert_eq!(shell.feed(b"\x1b[D\x1bOH!"), ("!".to_owned(), None));
        let (shown, exit) = shell.feed(b"\r");
        assert_eq!(shown, "\r\nhi!\r\nroot@ap-garden:~# ");
        assert_eq!(exit, None);
        assert_eq!(
            shell.feed(b"uname -a\r").0,
            "uname -a\r\nLinux ap-garden 6.6.110 #0 SMP aarch64 GNU/Linux\r\nroot@ap-garden:~# "
        );
        assert_eq!(shell.feed(b"vi\x03").0, "vi^C\r\nroot@ap-garden:~# ");
        assert!(
            shell
                .feed(b"ls\r")
                .0
                .contains("-ash: ls: the demo's devices run no other commands")
        );
        assert_eq!(shell.feed(b"exit 3\r").1, Some(3));
        assert_eq!(Shell::new(host()).feed(b"\x04").1, Some(0));
    }

    async fn read(terminal: &mut Terminal) -> Output {
        tokio::time::timeout(Duration::from_secs(5), terminal.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn the_shell_greets_answers_and_exits() {
        let mut terminal = open(host(), PtySize::DEFAULT);
        let Output::Stdout(hello) = read(&mut terminal).await else {
            panic!("a greeting")
        };
        let hello = String::from_utf8_lossy(&hello).into_owned();
        assert!(
            hello.contains("OpenWrt 24.10.4, r28959-29397011cc"),
            "{hello}"
        );
        assert!(hello.ends_with("root@ap-garden:~# "), "{hello}");
        terminal.resize(PtySize {
            cols: 120,
            rows: 40,
        });
        terminal
            .write(Bytes::from_static(b"hostname\r"))
            .await
            .unwrap();
        assert_eq!(
            read(&mut terminal).await,
            Output::Stdout(Bytes::from_static(
                b"hostname\r\nap-garden\r\nroot@ap-garden:~# "
            ))
        );
        terminal.write(Bytes::from_static(b"exit\r")).await.unwrap();
        assert_eq!(
            read(&mut terminal).await,
            Output::Stdout(Bytes::from_static(b"exit\r\n"))
        );
        assert_eq!(read(&mut terminal).await, Output::Exit(ExitStatus::Code(0)));
    }
}
