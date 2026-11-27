//! The control channel's reads, for the fuzz target `iperf3` (the `fuzzing` feature).

use super::proto::{self, MAX_PARAMS, MAX_RESULTS, Params};

/// A client's control messages, read as the server reads them: the cookie, the parameters
/// (checked as a server allowing 60 s and 8 streams checks them) and its results.
pub(crate) async fn control(mut from: &[u8]) {
    if proto::read_cookie(&mut from).await.is_err() {
        return;
    }
    let Ok(params) = proto::read_json(&mut from, MAX_PARAMS).await else {
        return;
    };
    let _ = Params::parse(&params, 60, 8);
    let _ = proto::read_json(&mut from, MAX_RESULTS).await;
}
