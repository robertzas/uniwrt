//! The russh client handler: checks the host key against the pin and reports why a session
//! ended.

use std::{
    future::ready,
    sync::{Arc, Mutex, PoisonError},
};

use russh::{
    client::{DisconnectReason, Handler},
    keys::PublicKeyOrCertificate,
};
use tokio::sync::watch;

use crate::host_key::HostKey;

pub(super) struct Client {
    /// None when probing: no key is trusted, the presented one is only recorded.
    pub(super) pinned: Option<HostKey>,
    pub(super) presented: Arc<Mutex<Option<HostKey>>>,
    /// Set to why the session ended. The handler lives as long as the session, so receivers
    /// also see the sender drop when it ends without a reason.
    pub(super) closed: watch::Sender<Option<String>>,
}

impl Handler for Client {
    type Error = russh::Error;

    fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        ready(Ok(self.check(key)))
    }

    fn disconnected(
        &mut self,
        reason: DisconnectReason<Self::Error>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let (text, result) = match reason {
            DisconnectReason::ReceivedDisconnect(info) => (
                format!(
                    "the node disconnected ({:?}): {}",
                    info.reason_code, info.message
                ),
                Ok(()),
            ),
            DisconnectReason::Error(err) => (err.to_string(), Err(err)),
        };
        self.closed.send_replace(Some(text));
        ready(result)
    }
}

impl Client {
    /// Records the presented key; true if it is the pinned one.
    fn check(&self, key: &PublicKeyOrCertificate) -> bool {
        // OpenWrt nodes have plain host keys; certificates are never trusted (D-032).
        let PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            return false;
        };
        let Ok(presented) = HostKey::new(key.clone()) else {
            return false;
        };
        let trusted = self.pinned.as_ref() == Some(&presented);
        *self
            .presented
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(presented);
        trusted
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if self.closed.borrow().is_none() {
            self.closed
                .send_replace(Some("the connection closed".to_owned()));
        }
    }
}
