//! [`Api`]: what the handlers share: the store, the engine, the clock, the build identity, the
//! HTTPS certificate, the password hasher, the login limiter, the setup window and the signal
//! that closes the WS connections.

use std::{fmt, sync::Arc, time::Duration};

use tokio::sync::watch;
use uniwrt_core::{Clock, Timestamp};
use uniwrt_engine::Fleet;
use uniwrt_model::user::{Role, User};
use uniwrt_store::{Store, UserRepo as _};
use zeroize::Zeroizing;

use crate::{
    ApiError, BuildInfo, Certificate, PasswordCost, SETUP_WINDOW,
    auth::{limit::LoginLimiter, password, password::Passwords, setup::Setup},
};

/// What [`Api::new`] needs.
pub struct ApiConfig {
    pub store: Store,
    /// The engine, whatever its link type.
    pub fleet: Arc<dyn Fleet>,
    pub clock: Arc<dyn Clock>,
    pub build: BuildInfo,
    pub certificate: Arc<dyn Certificate>,
    pub password_cost: PasswordCost,
    /// How long setup stays open after a start without users ([`SETUP_WINDOW`]).
    pub setup_window: Duration,
}

impl ApiConfig {
    /// The production settings.
    pub fn new(
        store: Store,
        fleet: Arc<dyn Fleet>,
        clock: Arc<dyn Clock>,
        build: BuildInfo,
        certificate: Arc<dyn Certificate>,
    ) -> Self {
        Self {
            store,
            fleet,
            clock,
            build,
            certificate,
            password_cost: PasswordCost::default(),
            setup_window: SETUP_WINDOW,
        }
    }
}

/// The API's shared state. Clones share it.
#[derive(Clone)]
pub struct Api {
    inner: Arc<Inner>,
}

struct Inner {
    store: Store,
    fleet: Arc<dyn Fleet>,
    clock: Arc<dyn Clock>,
    build: BuildInfo,
    certificate: Arc<dyn Certificate>,
    passwords: Passwords,
    limiter: LoginLimiter,
    setup: Setup,
    /// True once the server stops: the WS connections close.
    closing: watch::Sender<bool>,
}

impl Api {
    /// Opens the setup window if the store has no users. Takes one password hash's time.
    pub async fn new(config: ApiConfig) -> Result<Self, ApiError> {
        let has_users = config.store.has_users().await?;
        let passwords = Passwords::new(config.password_cost)?;
        passwords.warm_up().await?;
        Ok(Self {
            inner: Arc::new(Inner {
                passwords,
                limiter: LoginLimiter::default(),
                setup: Setup::new(has_users, config.setup_window),
                store: config.store,
                fleet: config.fleet,
                closing: watch::Sender::new(false),
                clock: config.clock,
                build: config.build,
                certificate: config.certificate,
            }),
        })
    }

    /// Creates the first admin without setup or the password policy: demo mode's `demo` user.
    pub async fn create_admin(&self, username: &str, password: &str) -> Result<User, ApiError> {
        let username = password::username(username)?;
        let hash = self
            .passwords()
            .hash(Zeroizing::new(password.to_owned()))
            .await?;
        let user = self
            .store()
            .create_first_user(&username, Role::Admin, &hash)
            .await?;
        self.inner.setup.finish();
        Ok(user)
    }

    /// Whether first-run setup is still to be done (and its window open).
    pub fn awaiting_setup(&self) -> bool {
        self.inner.setup.state().0 == crate::auth::setup::SetupState::Open
    }

    /// Closes every WS connection (code 1001, going away), for a server that stops: it waits for
    /// HTTP requests, not for upgraded connections.
    pub fn close_sockets(&self) {
        self.inner.closing.send_replace(true);
    }

    pub(crate) fn store(&self) -> &Store {
        &self.inner.store
    }

    pub(crate) fn fleet(&self) -> &dyn Fleet {
        self.inner.fleet.as_ref()
    }

    pub(crate) fn closing(&self) -> watch::Receiver<bool> {
        self.inner.closing.subscribe()
    }

    pub(crate) fn now(&self) -> Timestamp {
        self.inner.clock.now()
    }

    pub(crate) fn build(&self) -> BuildInfo {
        self.inner.build
    }

    pub(crate) fn certificate(&self) -> &dyn Certificate {
        self.inner.certificate.as_ref()
    }

    pub(crate) fn passwords(&self) -> &Passwords {
        &self.inner.passwords
    }

    pub(crate) fn limiter(&self) -> &LoginLimiter {
        &self.inner.limiter
    }

    pub(crate) fn setup(&self) -> &Setup {
        &self.inner.setup
    }
}

impl fmt::Debug for Api {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Api")
            .field("store", &self.inner.store)
            .finish_non_exhaustive()
    }
}
