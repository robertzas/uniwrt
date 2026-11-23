//! Commands on session channels. Each channel gets two tasks: one feeds stdin under the
//! connection's flow control, the other drains every message the node sends, so a slow consumer
//! never stalls the connection (russh waits for a channel's buffer to have room).

use std::sync::Arc;

use russh::{
    Channel, ChannelMsg, ChannelReadHalf, ChannelWriteHalf,
    client::{Handle, Msg},
};
use tokio::sync::watch;

use super::client::Client;
use crate::{
    Error, Result,
    exec::{
        self, ExecStream, ExitStatus, Output, OutputSink, PtySize, STREAM_BUDGET, StdinSource,
        TERM, Terminal,
    },
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

/// Starts a login shell on a pseudo-terminal of `size` on a new channel. `what` names it in
/// errors.
pub(super) async fn shell(
    handle: &Handle<Client>,
    target: &str,
    size: PtySize,
    what: String,
) -> Result<Terminal> {
    let mut channel = handle
        .channel_open_session()
        .await
        .map_err(|err| Error::ssh(target, err))?;
    let (cols, rows) = (u32::from(size.cols), u32::from(size.rows));
    channel
        .request_pty(true, TERM, cols, rows, 0, 0, &[])
        .await
        .map_err(|err| Error::ssh(target, err))?;
    granted(&mut channel, &what, "a terminal").await?;
    channel
        .request_shell(true)
        .await
        .map_err(|err| Error::ssh(target, err))?;
    let first = granted(&mut channel, &what, "a shell").await?;
    let (read, write) = channel.split();
    let (terminal, source) = exec::terminal_pipe(what, STREAM_BUDGET, size);
    let write = Arc::new(write);
    tokio::spawn(feed_stdin(source.stdin, Arc::clone(&write)));
    tokio::spawn(follow_size(source.size, Arc::clone(&write)));
    tokio::spawn(drain(read, first, source.output, write));
    Ok(terminal)
}

/// Waits for the node's answer to a request. A message other than an answer means it was
/// granted (as an exec request's output may come first), and is handed back.
async fn granted(
    channel: &mut Channel<Msg>,
    what: &str,
    asked: &str,
) -> Result<Option<ChannelMsg>> {
    loop {
        return match channel.wait().await {
            Some(ChannelMsg::Success) => Ok(None),
            Some(ChannelMsg::WindowAdjusted { .. }) => continue,
            Some(ChannelMsg::Failure) => Err(Error::Failed {
                what: what.to_owned(),
                status: ExitStatus::Unknown,
                stderr: format!("the node refused {asked}"),
            }),
            None => Err(Error::Disconnected {
                what: what.to_owned(),
            }),
            other => Ok(other),
        };
    }
}

/// Tells the node each new window size. Once the [`Terminal`] is dropped, closes the channel:
/// the node hangs its shell up.
async fn follow_size(mut size: watch::Receiver<PtySize>, channel: Arc<ChannelWriteHalf<Msg>>) {
    while size.changed().await.is_ok() {
        let PtySize { cols, rows } = *size.borrow_and_update();
        let change = channel.window_change(u32::from(cols), u32::from(rows), 0, 0);
        if change.await.is_err() {
            return;
        }
    }
    let _ = channel.close().await;
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
