//! `uniwrt reset-admin [USERNAME]` (features/security.md, Users; D-153): a forgotten admin
//! password replaced by a random one, printed once. It opens the controller's database beside
//! the running controller, so no restart is needed; [`uniwrt_api::reset_admin`] does the work.

use std::sync::Arc;

use anyhow::{Context as _, bail};
use uniwrt_core::{Clock as _, SystemClock};
use uniwrt_store::{Store, controller_backup::DATABASE};

use crate::cli::ResetAdminArgs;

pub(crate) fn run(args: &ResetAdminArgs) -> anyhow::Result<()> {
    let data_dir = std::path::absolute(&args.data_dir)?;
    let database = data_dir.join(DATABASE);
    // Opening would create an empty database: a wrong directory should fail instead.
    if !database.is_file() {
        bail!(
            "no controller database at {}: point --data-dir (UNIWRT_DATA_DIR) at the controller's data directory",
            database.display()
        );
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("cannot start the async runtime")?;
    let reset = runtime.block_on(async {
        let store = Store::open(&database, Arc::new(SystemClock))
            .await
            .with_context(|| format!("cannot open {}", database.display()))?;
        let reset =
            uniwrt_api::reset_admin(&store, args.username.as_deref(), SystemClock.now()).await;
        store.close().await;
        anyhow::Ok(reset?)
    })?;
    println!(
        "The new password of {}:\n\n    {}\n\nSign in with it, then change it from your account menu. Their sessions ended.\nIf the sign-in page asks you to wait, earlier failed attempts are still counted: wait as it says.",
        reset.username,
        reset.password.as_str()
    );
    Ok(())
}
