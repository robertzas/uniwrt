//! A node's log read over its own SSH session (`logread`), for checks that must not trust
//! syslog, which any host on the LAN can forge (threat-model.md, §3): the upgrade's baseline and
//! its soak (D-150).

use uniwrt_model::log::LogEntry;

use crate::{ExecOutput, ExecRequest, events::logread_line};

/// The newest entries read: logd's ring buffer (64 KiB by default) holds a few hundred.
pub const MAX_LINES: u32 = 2000;
/// What [`MAX_LINES`] lines of logread take at most, with room to spare.
const MAX_OUTPUT: usize = 1 << 20;

/// `logread -t -l <MAX_LINES>`: the newest entries, as the agent's stream reads them.
pub fn request() -> ExecRequest {
    ExecRequest::new(["logread", "-t", "-l", &MAX_LINES.to_string()]).max_output(MAX_OUTPUT)
}

/// The entries of a successful [`request`], oldest first; lines that don't parse are left out.
pub fn parse(output: &ExecOutput) -> Vec<LogEntry> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(logread_line)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExitStatus;

    #[test]
    fn logread_s_lines_become_entries() {
        let output = ExecOutput {
            status: ExitStatus::Code(0),
            stdout: b"Thu Oct 29 03:40:30 2026 [1793245230.849] kern.err kernel: [ 12.5] mt7915e 0000:01:00.0: Message 00 (seq 3) timeout\nnot a line\nThu Oct 29 03:40:31 2026 [1793245231.000] daemon.notice netifd: wan is up\n".to_vec(),
            stderr: Vec::new(),
        };
        let entries = parse(&output);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].program.as_deref(), Some("kernel"));
        assert_eq!(entries[1].message, "wan is up");
        assert_eq!(request().argv, ["logread", "-t", "-l", "2000"]);
    }
}
