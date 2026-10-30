//! The setup window (D-040, owner's choice): a controller that starts without users accepts
//! first-run setup for [`SETUP_WINDOW`], so that someone on the LAN cannot claim one that was
//! forgotten; restarting it opens the window again.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use serde::Serialize;
use tokio::time::Instant;
use ts_rs::TS;

pub const SETUP_WINDOW: Duration = Duration::from_mins(10);

/// Where first-run setup stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub(crate) enum SetupState {
    /// No users yet, and the window is open.
    Open,
    /// No users yet, but the window has closed: restart the controller to reopen it.
    Closed,
    /// The admin exists; log in.
    Done,
}

pub(crate) struct Setup {
    /// When the window opened; `None` if users existed at start.
    opened: Option<Instant>,
    window: Duration,
    done: AtomicBool,
}

impl Setup {
    pub(crate) fn new(has_users: bool, window: Duration) -> Self {
        Self {
            opened: (!has_users).then(Instant::now),
            window,
            done: AtomicBool::new(has_users),
        }
    }

    /// The state, and while open, how long it stays open.
    pub(crate) fn state(&self) -> (SetupState, Option<Duration>) {
        if self.done.load(Ordering::Acquire) {
            return (SetupState::Done, None);
        }
        let left = self
            .opened
            .and_then(|opened| self.window.checked_sub(opened.elapsed()))
            .filter(|left| !left.is_zero());
        match left {
            Some(left) => (SetupState::Open, Some(left)),
            None => (SetupState::Closed, None),
        }
    }

    pub(crate) fn finish(&self) {
        self.done.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{SETUP_WINDOW, Setup, SetupState};

    #[tokio::test(start_paused = true)]
    async fn the_window_closes_ten_minutes_after_a_start_without_users() {
        let setup = Setup::new(false, SETUP_WINDOW);
        assert_eq!(setup.state(), (SetupState::Open, Some(SETUP_WINDOW)));
        tokio::time::advance(Duration::from_mins(9)).await;
        assert_eq!(
            setup.state(),
            (SetupState::Open, Some(Duration::from_mins(1)))
        );
        tokio::time::advance(Duration::from_mins(1)).await;
        assert_eq!(setup.state(), (SetupState::Closed, None));
        setup.finish();
        assert_eq!(setup.state(), (SetupState::Done, None));
        assert_eq!(
            Setup::new(true, SETUP_WINDOW).state(),
            (SetupState::Done, None)
        );
    }
}
