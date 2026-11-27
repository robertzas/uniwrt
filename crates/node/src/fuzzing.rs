//! Entry points of the fuzz targets (`fuzz/`; M9.3b, D-150), behind the `fuzzing` feature: the
//! parsers of what a node sends that the crate doesn't export. They return nothing: a panic, a
//! hang or an allocation past libFuzzer's limit is the finding.

use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};

use uniwrt_model::{facts::NodeFacts, wifi::Band};

use crate::{
    events, facts,
    lines::LineBuf,
    proto::{
        Message,
        ops::{Collected, Scanned},
    },
};

/// The agent's stdout as [`Agent`](crate::Agent) reads it: `bytes` in chunks of `chunk`,
/// split into lines of at most `max` bytes (a longer one ends the stream), each line classified
/// ([`Message::parse`]): a push parsed ([`events::parse`]), an answer read as a `collect`
/// answer applied to facts and as a `scan` answer.
pub fn agent_stream(chunk: usize, max: usize, bytes: &[u8]) {
    let mut lines = LineBuf::new(max);
    for part in bytes.chunks(chunk.max(1)) {
        lines.push(part);
        loop {
            match lines.next_line() {
                Ok(Some(line)) => agent_line(line),
                Ok(None) => break,
                Err(_) => return,
            }
        }
    }
}

fn agent_line(line: &[u8]) {
    match Message::parse(line) {
        Ok(Message::Event(event)) => {
            let _ = events::parse(&event);
        }
        Ok(Message::Answer {
            result: Ok(raw), ..
        }) => {
            if let Ok(collected) = serde_json::from_str::<Collected>(raw.get()) {
                let _ = facts::apply(&collected, &mut NodeFacts::default());
            }
            if let Ok(scanned) = serde_json::from_str::<Scanned>(raw.get()) {
                for band in [Band::G2, Band::G5, Band::G6, Band::G60] {
                    let _ = facts::scan::heard(&scanned, band);
                }
            }
        }
        _ => {}
    }
}

/// An iperf3 client's control messages, as the controller's server reads them.
pub fn iperf3_control(bytes: &[u8]) {
    block_on(crate::iperf3::fuzz::control(bytes));
}

/// Runs a future whose reads never wait (from a slice) to its end.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
}
