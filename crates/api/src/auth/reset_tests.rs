use std::sync::Arc;

use uniwrt_core::{ManualClock, Timestamp};
use uniwrt_model::user::{AuditAction, Role};
use uniwrt_store::{AuditRepo as _, NewSession, SessionRepo as _, Store, UserRepo as _};
use zeroize::Zeroizing;

use super::{ALPHABET, ResetError, generate, reset_admin};
use crate::auth::password::{PasswordCost, Passwords};

const AT: Timestamp = Timestamp::from_unix_secs(1_793_318_400);

async fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("uniwrt.db"), Arc::new(ManualClock::new(AT)))
        .await
        .unwrap();
    (dir, store)
}

#[test]
fn passwords_are_four_groups_of_five_unambiguous_characters() {
    let password = generate().unwrap();
    let groups: Vec<_> = password.split('-').collect();
    assert_eq!(groups.len(), 4, "{}", password.as_str());
    assert!(
        groups
            .iter()
            .all(|group| group.len() == 5 && group.bytes().all(|b| ALPHABET.contains(&b)))
    );
    assert_ne!(generate().unwrap(), password);
}

#[tokio::test]
async fn the_only_admin_gets_a_new_password_and_loses_their_sessions() {
    let (_dir, store) = store().await;
    assert!(matches!(
        reset_admin(&store, None, AT).await,
        Err(ResetError::NoUsers)
    ));
    let admin = store
        .create_first_user("admin", Role::Admin, "old hash")
        .await
        .unwrap();
    store.create_user("bob", Role::Viewer, "h").await.unwrap();
    store
        .create_session(NewSession {
            token_sha256: [7; 32],
            user: admin.id,
            expires_at: Timestamp::from_unix_secs(4_000_000_000),
            ip: None,
            user_agent: None,
        })
        .await
        .unwrap();

    let reset = reset_admin(&store, None, AT).await.unwrap();
    assert_eq!(reset.username, "admin");
    let (_, hash) = store.credentials("admin").await.unwrap().unwrap();
    let passwords = Passwords::new(PasswordCost::CHEAPEST).unwrap();
    assert!(
        passwords
            .verify(reset.password.clone(), Some(hash))
            .await
            .unwrap()
    );
    assert!(store.session([7; 32]).await.unwrap().is_none());
    let log = store.audit_log(10).await.unwrap();
    assert_eq!(
        (
            log[0].action,
            log[0].username.as_deref(),
            log[0].ip.as_deref()
        ),
        (AuditAction::ResetAdmin, Some("admin"), None)
    );

    // A named admin, by any case; not a viewer or a stranger.
    assert_eq!(
        reset_admin(&store, Some(" Admin"), AT)
            .await
            .unwrap()
            .username,
        "admin"
    );
    assert!(matches!(
        reset_admin(&store, Some("bob"), AT).await,
        Err(ResetError::Viewer(name)) if name == "bob"
    ));
    assert!(matches!(
        reset_admin(&store, Some("carol"), AT).await,
        Err(ResetError::Unknown(_))
    ));

    // Several admins: one must be named.
    store.create_user("carol", Role::Admin, "h").await.unwrap();
    let err = reset_admin(&store, None, AT).await.err().unwrap();
    assert_eq!(
        err.to_string(),
        "there are several admins (admin, carol): name the one to reset"
    );
    let _: Zeroizing<String> = reset_admin(&store, Some("carol"), AT)
        .await
        .unwrap()
        .password;
}
