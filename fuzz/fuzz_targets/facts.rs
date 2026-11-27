//! The collect sets' parsers (threat-model.md §4): the input as a `collect` answer applied to
//! a node's facts, and as the sources of every set.
#![no_main]

use libfuzzer_sys::fuzz_target;
use serde_json::value::RawValue;
use uniwrt_model::facts::NodeFacts;
use uniwrt_node::{
    facts::{self, SetFacts},
    proto::ops::{Collected, Set},
};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(collected) = serde_json::from_str::<Collected>(text) {
        let _ = facts::apply(&collected, &mut NodeFacts::default());
    }
    if let Ok(raw) = serde_json::from_str::<Box<RawValue>>(text) {
        for set in Set::ALL {
            if let Ok(parsed) = SetFacts::parse(set, &raw) {
                parsed.apply(&mut NodeFacts::default());
            }
        }
    }
});
