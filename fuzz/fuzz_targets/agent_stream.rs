//! The agent's stdout (threat-model.md §4): the line decoder, each line classified (hello,
//! answer, push), every push parsed, answers read as `collect` and `scan` answers. Twice: in
//! chunks whose size the input's length picks, and with lines of at most 64 bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;
use uniwrt_node::{fuzzing::agent_stream, proto::MAX_LINE};

fuzz_target!(|data: &[u8]| {
    agent_stream(data.len() % 97 + 1, MAX_LINE, data);
    agent_stream(data.len() % 13 + 1, 64, data);
});
