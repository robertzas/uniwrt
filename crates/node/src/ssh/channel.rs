//! Commands on session channels. Each channel gets two tasks: one feeds stdin under the
//! connection's flow control, the other drains every message the node sends, so a slow consumer
//! never stalls the connection (russh waits for a channel's buffer to have room).

use std::sync::Arc;

use russh::{
    ChannelMsg, ChannelReadHalf, ChannelWriteHalf,
    client::{Handle, Msg},
};

use super::client::Client;
use crate::{
    Error, Result,
    exec::{self, ExecStream, ExitStatus, Output, OutputSink, STREAM_BUDGET, StdinSource},
};

/// Starts `command` on a new channel. `what` names it in errors.
pub(super) async fn open(
    handle: &Handle<Client>,
    target: &str,
    command: String,
    what: String,
) -> Result<ExecStream> {
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|err| Error::ssh(target, err))?;
    channel
        .exec(true, command)
        .await
        .map_err(|err| Error::ssh(target, err))?;
    let (mut read, write) = channel.split();
    // The node answers the exec request before the command says anything.
    let first = match read.wait().await {
        Some(ChannelMsg::Success) => None,
        Some(ChannelMsg::Failure) => {
            return Err(Error::Failed {
                what,
                status: ExitStatus::Unknown,
                stderr: "the node refused to run it".to_owned(),
            });
        }
        None => return Err(Error::Disconnected { what }),
        other => other,
    };
    let (stream, source) = exec::pipe(what, STREAM_BUDGET);
    let write = Arc::new(write);
    tokio::spawn(feed_stdin(source.stdin, Arc::clone(&write)));
    tokio::spawn(drain(read, first, source.output, write));
    Ok(stream)
}

async fn feed_stdin(mut stdin: StdinSource, channel: Arc<ChannelWriteHalf<Msg>>) {
    while let Some(data) = stdin.next().await {
        if channel.data_bytes(data).await.is_err() {
            return;
        }
    }
    let _ = channel.eof().await;
}

async fn drain(
    mut read: ChannelReadHalf,
    mut first: Option<ChannelMsg>,
    sink: OutputSink,
    channel: Arc<ChannelWriteHalf<Msg>>,
) {
    let mut exited = false;
    loop {
        let message = match first.take() {
            Some(message) => Some(message),
            None => read.wait().await,
        };
        let Some(message) = message else {
            // The connection is gone: what is in flight fails fast.
            sink.fail(Error::Disconnected {
                what: "the command".to_owned(),
            });
            return;
        };
        let wanted = match message {
            ChannelMsg::Data { data } => sink.push(Output::Stdout(data)),
            ChannelMsg::ExtendedData { data, ext: 1 } => sink.push(Output::Stderr(data)),
            ChannelMsg::ExitStatus { exit_status } => {
                exited = true;
                sink.push(Output::Exit(ExitStatus::Code(exit_status)))
            }
            ChannelMsg::ExitSignal { signal_name, .. } => {
                exited = true;
                sink.push(Output::Exit(ExitStatus::Signal(format!("{signal_name:?}"))))
            }
            ChannelMsg::Close => break,
            _ => true,
        };
        if !wanted {
            // Nobody reads any more: close the channel, and discard what is still on its way.
            let _ = channel.close().await;
            while read.wait().await.is_some() {}
            return;
        }
    }
    if !exited {
        sink.push(Output::Exit(ExitStatus::Unknown));
    }
}
