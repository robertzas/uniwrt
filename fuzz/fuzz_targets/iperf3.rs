//! The controller's iperf3 server (threat-model.md §3): a client's control messages, the
//! cookie, the parameters behind their length and the results.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| uniwrt_node::fuzzing::iperf3_control(data));
