//! The database's compatibility across versions (D-152): frozen migrations, and every fixture
//! upgraded to the latest schema and read back.

use std::{env, fs, path::Path};

use data_encoding::HEXLOWER;
use ring::digest::{SHA256, digest};
use rusqlite::Connection;

use super::{dump, fixtures, load, seed};
use crate::{Store, migrate};

fn dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/compat"))
}

#[test]
fn released_migrations_are_frozen() {
    let frozen: Vec<(&str, &str)> = include_str!("../../migrations/frozen.sha256")
        .lines()
        .map(|line| line.split_once("  ").expect("a `<sha256>  <file>` line"))
        .collect();
    let sources: Vec<_> = migrate::sources().collect();
    assert!(
        frozen.len() <= sources.len(),
        "a released migration is gone: frozen.sha256 lists {}, the store has {}",
        frozen.len(),
        sources.len()
    );
    for (at, (name, sql)) in sources.iter().enumerate() {
        let file = format!("{name}.sql");
        let hash = HEXLOWER.encode(digest(&SHA256, sql.as_bytes()).as_ref());
        let Some((want, listed)) = frozen.get(at) else {
            panic!(
                "{file} is new: append this line to crates/store/migrations/frozen.sha256 \
                 (released, it is frozen; D-152):\n{hash}  {file}"
            );
        };
        assert_eq!(
            *listed,
            file,
            "frozen.sha256's line {} (append-only)",
            at + 1
        );
        assert_eq!(
            *want, hash,
            "{file} changed after its release: migrations are frozen (D-152); add one instead"
        );
    }
}

#[tokio::test]
async fn every_fixture_upgrades_to_the_latest_schema_and_reads_back() {
    let latest = migrate::latest();
    if !fixtures(dir()).iter().any(|(schema, _)| *schema == latest) {
        assert!(
            env::var_os("UNIWRT_BLESS").is_some(),
            "schema {latest} has no fixture: run `UNIWRT_BLESS=1 cargo nextest run -p \
             uniwrt-store compat` to write crates/store/fixtures/compat/schema-{latest:04}.sql \
             from the seed, and commit it (D-152)"
        );
        bless(latest).await;
    }
    for (schema, name) in fixtures(dir()) {
        let rows = fs::read_to_string(dir().join(&name)).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("uniwrt.db");
        load(&path, schema, &rows).unwrap_or_else(|err| panic!("{name} doesn't load: {err:?}"));
        let store = Store::open(&path, seed::clock())
            .await
            .unwrap_or_else(|err| panic!("{name} doesn't upgrade: {err}"));
        seed::check(&store).await;
        let broken: i64 = store
            .read(|conn| {
                Ok(
                    conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                        row.get(0)
                    })?,
                )
            })
            .await
            .unwrap();
        assert_eq!(broken, 0, "{name}: rows whose foreign keys point nowhere");
        store.close().await;
    }
}

#[tokio::test]
async fn a_fixture_reloads_as_it_was_dumped() {
    let (schema, name) = fixtures(dir()).pop().expect("a fixture");
    let rows = fs::read_to_string(dir().join(&name)).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("uniwrt.db");
    load(&path, schema, &rows).unwrap();
    let dumped = dump(&Connection::open(&path).unwrap()).unwrap();
    assert_eq!(body(&rows), dumped);
}

/// Writes the fixture of schema `latest` from the seed.
async fn bless(latest: usize) {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("uniwrt.db");
    let store = Store::open(&path, seed::clock()).await.unwrap();
    seed::seed(&store).await;
    store.close().await;
    let rows = dump(&Connection::open(&path).unwrap()).unwrap();
    let file = dir().join(format!("schema-{latest:04}.sql"));
    fs::create_dir_all(dir()).unwrap();
    fs::write(
        &file,
        format!(
            "-- uniwrt's database at schema {latest}, as the compatibility seed wrote it \
             (crates/store/src/compat; D-152).\n-- Frozen: every later uniwrt must upgrade and \
             read it. Never edit or rewrite it.\n{rows}"
        ),
    )
    .unwrap();
}

/// A fixture's rows without its comment lines.
fn body(fixture: &str) -> String {
    fixture
        .lines()
        .filter(|line| !line.starts_with("--"))
        .flat_map(|line| [line, "\n"])
        .collect()
}
