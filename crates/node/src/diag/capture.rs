//! A packet capture's command and its cut (D-142): `tcpdump -U -w -` writes the `.pcap` to
//! stdout, its header at once and each packet as it is captured, under [`CAPTURE`], the
//! watchdog's script (a closed channel ends it) with a setup of its own: the controller's SSH
//! session, read from dropbear's `$SSH_CONNECTION` (or `$SSH_CLIENT`), is left out of the
//! capture as `not (tcp and host <controller> and port <its port> and port <ssh>)`, appended to
//! the admin's filter, which reaches tcpdump after `--` as one argument (checked on OpenWrt
//! 24.10 and 25.12 with `tcpdump-mini`). Without it the capture would capture itself.
//!
//! [`PcapCut`] passes the stream through up to a size cap, cut before the first packet that
//! would pass it, so the download stays a valid `.pcap`.

use uniwrt_model::diag::capture::{CaptureQuery, MAX_SNAPLEN};

/// Runs `"$@" -- "<filter>"` under the watchdog, its first argument the admin's filter (empty
/// for none) and the controller's session left out.
pub const CAPTURE: &str = guarded!(
    "user=$1; shift
c=${SSH_CONNECTION:-${SSH_CLIENT:-}}
f=$user
if [ -n \"$c\" ]; then
\tip=${c%% *}; ports=${c#* }
\tip=${ip%%[%]*}; ip=${ip#::ffff:}
\town=\"not (tcp and host $ip and port ${ports%% *} and port ${c##* })\"
\tif [ -n \"$user\" ]; then f=\"($user) and $own\"; else f=$own; fi
fi
[ -z \"$f\" ] || set -- \"$@\" -- \"$f\"
"
);

/// The command that captures as `query` asks (validated first), under [`CAPTURE`].
pub fn command(query: &CaptureQuery) -> Vec<String> {
    let mut argv: Vec<String> = ["sh", "-c", CAPTURE, "sh"].map(str::to_owned).into();
    argv.push(query.filter().unwrap_or_default().to_owned());
    argv.extend(["tcpdump", "-i"].map(str::to_owned));
    argv.push(query.interface.clone());
    if let Some(snaplen) = query.snaplen {
        argv.push("-s".to_owned());
        argv.push(snaplen.to_string());
    }
    argv.extend(["-U", "-w", "-"].map(str::to_owned));
    argv
}

/// The bytes of a pcap file's header.
const FILE_HEADER: usize = 24;
/// The bytes of a packet record's header.
const RECORD_HEADER: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Reading the file's header.
    File,
    /// Reading a record's header.
    Record,
    /// Passing a record's data: the bytes left.
    Data(u64),
    /// Not a pcap file: passed through up to the cap.
    Raw,
    /// The cap is reached: nothing more passes.
    Done,
}

/// Passes a pcap stream through up to `max` bytes, ending before the first packet that would
/// pass it.
#[derive(Debug)]
pub struct PcapCut {
    max: u64,
    sent: u64,
    packets: u64,
    pending: Vec<u8>,
    state: State,
    big_endian: bool,
}

impl PcapCut {
    pub const fn new(max: u64) -> Self {
        Self {
            max,
            sent: 0,
            packets: 0,
            pending: Vec::new(),
            state: State::File,
            big_endian: false,
        }
    }

    /// Whether the cap is reached: nothing more will pass.
    pub fn done(&self) -> bool {
        self.state == State::Done
    }

    /// The bytes passed so far.
    pub const fn bytes(&self) -> u64 {
        self.sent
    }

    /// The packets passed so far.
    pub const fn packets(&self) -> u64 {
        self.packets
    }

    /// What passes of `data`, the stream's next bytes.
    pub fn push(&mut self, mut data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        while !data.is_empty() {
            match self.state {
                State::File | State::Record => {
                    let size = if self.state == State::File {
                        FILE_HEADER
                    } else {
                        RECORD_HEADER
                    };
                    let take = (size - self.pending.len()).min(data.len());
                    self.pending.extend_from_slice(&data[..take]);
                    data = &data[take..];
                    if self.pending.len() == size {
                        self.header(&mut out);
                    }
                }
                State::Data(left) => {
                    let take = usize::try_from(left).unwrap_or(usize::MAX).min(data.len());
                    self.pass(&mut out, &data[..take]);
                    data = &data[take..];
                    let left = left - take as u64;
                    self.state = if left == 0 {
                        State::Record
                    } else {
                        State::Data(left)
                    };
                }
                State::Raw => {
                    let room = usize::try_from(self.max - self.sent).unwrap_or(usize::MAX);
                    let take = room.min(data.len());
                    self.pass(&mut out, &data[..take]);
                    data = &data[take..];
                    if self.sent >= self.max {
                        self.state = State::Done;
                    }
                }
                State::Done => break,
            }
        }
        out
    }

    /// Acts on the header in `pending`: the file's or a record's.
    fn header(&mut self, out: &mut Vec<u8>) {
        let header = std::mem::take(&mut self.pending);
        if self.state == State::File {
            let magic = [header[0], header[1], header[2], header[3]];
            self.state = match magic {
                [0xd4, 0xc3, 0xb2, 0xa1] | [0x4d, 0x3c, 0xb2, 0xa1] => State::Record,
                [0xa1, 0xb2, 0xc3, 0xd4] | [0xa1, 0xb2, 0x3c, 0x4d] => {
                    self.big_endian = true;
                    State::Record
                }
                _ => State::Raw,
            };
            if self.state == State::Raw {
                let room = usize::try_from(self.max).unwrap_or(usize::MAX);
                let take = room.min(header.len());
                self.pass(out, &header[..take]);
            } else {
                self.pass(out, &header);
            }
            return;
        }
        let len = [header[8], header[9], header[10], header[11]];
        let len = u64::from(if self.big_endian {
            u32::from_be_bytes(len)
        } else {
            u32::from_le_bytes(len)
        });
        // A record longer than any snapshot length means the stream is not what it claims.
        if len > u64::from(MAX_SNAPLEN) || self.sent + RECORD_HEADER as u64 + len > self.max {
            self.state = State::Done;
            return;
        }
        self.pass(out, &header);
        self.packets += 1;
        self.state = if len == 0 {
            State::Record
        } else {
            State::Data(len)
        };
    }

    fn pass(&mut self, out: &mut Vec<u8>, data: &[u8]) {
        out.extend_from_slice(data);
        self.sent += data.len() as u64;
    }
}

#[cfg(test)]
mod tests;
