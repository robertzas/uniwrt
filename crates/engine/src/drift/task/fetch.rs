//! The watcher's reads, each in a task of its own that hands its result back as a command:
//! the files whose hash is new, and the agent's hashes after a job.

use uniwrt_core::NodeId;
use uniwrt_node::{
    Error as NodeError, NodeLink,
    facts::SetFacts,
    proto::ops::{Collect, ReadFile, Set, code},
};
use uniwrt_store::DriftRepo;

use super::Task;
use crate::drift::Command;

/// One file a read task got: the hash the agent reported when it was asked for (`None`: not
/// listed), and the file (`None`: gone) or why it couldn't be read.
pub(crate) struct Reading {
    pub(super) package: String,
    pub(super) reported: Option<String>,
    pub(super) text: Result<Option<String>, String>,
}

impl<L: NodeLink, R: DriftRepo + Clone + 'static> Task<L, R> {
    /// Asks the node's agent for its hashes now (they changed with the job), then looks at it;
    /// with the hashes known when the agent can't answer.
    pub(super) fn collect(&mut self, node: NodeId) {
        let Some(link) = self.engine.link(node) else {
            return;
        };
        let commands = self.commands.clone();
        let timeout = self.config.read_timeout;
        tokio::spawn(async move {
            let op = Collect {
                sets: vec![Set::Config],
            };
            let hashes = match link.request(&op, timeout).await {
                Ok(collected) => collected.sets.get(&Set::Config).and_then(|raw| {
                    match SetFacts::parse(Set::Config, raw) {
                        Ok(SetFacts::Config(hashes)) => Some(hashes),
                        _ => None,
                    }
                }),
                Err(_) => None,
            };
            let _ = commands.send(Command::Hashes { node, hashes }).await;
        });
    }

    /// Reads the files in a task of its own, which hands them back as [`Command::Read`].
    pub(super) fn read(&mut self, node: NodeId, reads: Vec<(String, Option<String>)>) {
        let Some(link) = self.engine.link(node) else {
            return;
        };
        self.reading.insert(node);
        let commands = self.commands.clone();
        let timeout = self.config.read_timeout;
        tokio::spawn(async move {
            let mut readings = Vec::with_capacity(reads.len());
            for (package, reported) in reads {
                let op = ReadFile::new(format!("/etc/config/{package}"));
                let text = match link.request(&op, timeout).await {
                    Ok(file) if file.truncated => Err("the file is larger than 1 MiB".to_owned()),
                    Ok(file) => Ok(Some(file.data)),
                    Err(NodeError::Remote { code: c, .. }) if c == code::NOT_FOUND => Ok(None),
                    Err(err) => Err(err.to_string()),
                };
                readings.push(Reading {
                    package,
                    reported,
                    text,
                });
            }
            let _ = commands.send(Command::Read { node, readings }).await;
        });
    }
}
