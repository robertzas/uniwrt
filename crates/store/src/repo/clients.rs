//! [`ClientRepo`] on SQLite: the `clients` and `client_sessions` tables.

use rusqlite::{Connection, Row, params};
use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_model::client::{ClientRecord, ClientSession};

use super::ClientRepo;
use crate::{
    Result, Store,
    codec::{enum_column, enum_text, mac_column, timestamp_column},
};

const SESSION_COLUMNS: &str = "mac, started, ended, ended_by, node, port, medium, band, ssid";

impl ClientRepo for Store {
    async fn clients(&self) -> Result<Vec<ClientRecord>> {
        self.read(|conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT mac, alias, hostname, first_seen, last_seen FROM clients ORDER BY mac",
                )?
                .query_map([], |row| {
                    Ok(ClientRecord {
                        mac: mac_column(row, 0)?,
                        alias: row.get(1)?,
                        hostname: row.get(2)?,
                        first_seen: timestamp_column(row, 3)?,
                        last_seen: timestamp_column(row, 4)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn save_clients(&self, records: Vec<ClientRecord>) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        self.write(move |conn| {
            for record in &records {
                upsert(conn, record, false)?;
            }
            Ok(())
        })
        .await
    }

    async fn save_alias(&self, record: ClientRecord) -> Result<()> {
        self.write(move |conn| upsert(conn, &record, true)).await
    }

    async fn save_sessions(&self, sessions: Vec<ClientSession>) -> Result<()> {
        if sessions.is_empty() {
            return Ok(());
        }
        self.write(move |conn| {
            let mut insert = conn.prepare_cached(&format!(
                "INSERT INTO client_sessions ({SESSION_COLUMNS})
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (mac, started) DO UPDATE SET
                 ended = excluded.ended, ended_by = excluded.ended_by, node = excluded.node,
                 port = excluded.port, medium = excluded.medium, band = excluded.band,
                 ssid = excluded.ssid"
            ))?;
            for session in &sessions {
                insert.execute(params![
                    session.mac.to_string(),
                    session.start.as_unix_ms(),
                    session.end.map(Timestamp::as_unix_ms),
                    session.ended_by.map(enum_text),
                    session.node.map(NodeId::get),
                    session.port,
                    enum_text(session.medium),
                    session.band.map(enum_text),
                    session.ssid,
                ])?;
            }
            Ok(())
        })
        .await
    }

    async fn client_sessions(&self, mac: MacAddr, since: Timestamp) -> Result<Vec<ClientSession>> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(&format!(
                    "SELECT {SESSION_COLUMNS} FROM client_sessions
                     WHERE mac = ?1 AND (ended IS NULL OR ended >= ?2) ORDER BY started"
                ))?
                .query_map(params![mac.to_string(), since.as_unix_ms()], session)?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn latest_sessions(&self) -> Result<Vec<ClientSession>> {
        self.read(|conn| {
            Ok(conn
                .prepare_cached(&format!(
                    "SELECT {SESSION_COLUMNS} FROM client_sessions AS s
                     WHERE started = (SELECT max(started) FROM client_sessions WHERE mac = s.mac)
                     ORDER BY mac"
                ))?
                .query_map([], session)?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn prune_clients(&self, before: Timestamp) -> Result<(usize, usize)> {
        self.write(move |conn| {
            let before = before.as_unix_ms();
            let gone = "SELECT mac FROM clients WHERE alias IS NULL AND last_seen < ?1";
            let sessions = conn
                .prepare_cached(&format!(
                    "DELETE FROM client_sessions WHERE ended < ?1 OR mac IN ({gone})"
                ))?
                .execute([before])?;
            let clients = conn
                .prepare_cached("DELETE FROM clients WHERE alias IS NULL AND last_seen < ?1")?
                .execute([before])?;
            Ok((clients, sessions))
        })
        .await
    }
}

/// Saves a record: a new one whole, an existing one's hostname (when known), the earlier first
/// and the later last sighting, and its alias only `with_alias`.
fn upsert(conn: &Connection, record: &ClientRecord, with_alias: bool) -> Result<()> {
    let alias = if with_alias {
        "alias = excluded.alias,"
    } else {
        ""
    };
    conn.prepare_cached(&format!(
        "INSERT INTO clients (mac, alias, hostname, first_seen, last_seen)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (mac) DO UPDATE SET {alias}
         hostname = coalesce(excluded.hostname, clients.hostname),
         first_seen = min(clients.first_seen, excluded.first_seen),
         last_seen = max(clients.last_seen, excluded.last_seen)"
    ))?
    .execute(params![
        record.mac.to_string(),
        record.alias,
        record.hostname,
        record.first_seen.as_unix_ms(),
        record.last_seen.as_unix_ms(),
    ])?;
    Ok(())
}

fn session(row: &Row<'_>) -> rusqlite::Result<ClientSession> {
    Ok(ClientSession {
        mac: mac_column(row, 0)?,
        start: timestamp_column(row, 1)?,
        end: row.get::<_, Option<i64>>(2)?.map(Timestamp::from_unix_ms),
        ended_by: optional_enum(row, 3)?,
        node: row.get::<_, Option<i64>>(4)?.map(NodeId::new),
        port: row.get(5)?,
        medium: enum_column(row, 6)?,
        band: optional_enum(row, 7)?,
        ssid: row.get(8)?,
    })
}

fn optional_enum<T: serde::de::DeserializeOwned>(
    row: &Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<T>> {
    match row.get_ref(index)? {
        rusqlite::types::ValueRef::Null => Ok(None),
        _ => enum_column(row, index).map(Some),
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_core::{MacAddr, NodeId, Timestamp};
    use uniwrt_model::{
        client::{ClientMedium, ClientRecord, ClientSession, SessionEnd},
        wifi::Band,
    };

    use crate::{ClientRepo as _, testing};

    fn mac(last: u8) -> MacAddr {
        MacAddr::new([0xf0, 0x18, 0x98, 0, 0, last])
    }

    fn ms(ms: i64) -> Timestamp {
        Timestamp::from_unix_ms(ms)
    }

    fn record(last: u8, seen: (i64, i64)) -> ClientRecord {
        ClientRecord {
            mac: mac(last),
            alias: None,
            hostname: Some(format!("host{last}")),
            first_seen: ms(seen.0),
            last_seen: ms(seen.1),
        }
    }

    fn session(last: u8, start: i64, end: Option<i64>) -> ClientSession {
        ClientSession {
            mac: mac(last),
            node: Some(NodeId::new(2)),
            port: Some("phy1-ap0".to_owned()),
            medium: ClientMedium::Wifi,
            band: Some(Band::G5),
            ssid: Some("home".to_owned()),
            start: ms(start),
            end: end.map(ms),
            ended_by: end.map(|_| SessionEnd::Left),
        }
    }

    #[tokio::test]
    async fn records_merge_and_keep_their_alias() {
        let (_dir, store, _) = testing::open().await;
        store
            .save_clients(vec![record(1, (100, 200))])
            .await
            .unwrap();
        let named = ClientRecord {
            alias: Some("Anna's phone".to_owned()),
            ..record(1, (100, 250))
        };
        store.save_alias(named).await.unwrap();
        // A later save without the alias keeps it, and the widest sightings.
        let later = ClientRecord {
            hostname: None,
            ..record(1, (150, 300))
        };
        store
            .save_clients(vec![later, record(2, (10, 20))])
            .await
            .unwrap();
        let clients = store.clients().await.unwrap();
        assert_eq!(
            clients[0],
            ClientRecord {
                alias: Some("Anna's phone".to_owned()),
                ..record(1, (100, 300))
            }
        );
        assert_eq!(clients[1], record(2, (10, 20)));
        store.save_alias(record(1, (100, 300))).await.unwrap();
        assert_eq!(store.clients().await.unwrap()[0].alias, None);
    }

    #[tokio::test]
    async fn sessions_update_by_start_and_read_back_by_time() {
        let (_dir, store, _) = testing::open().await;
        store
            .save_sessions(vec![session(1, 100, Some(200)), session(1, 300, None)])
            .await
            .unwrap();
        let roamed = ClientSession {
            ended_by: Some(SessionEnd::Roamed),
            ..session(1, 100, Some(200))
        };
        let wired = ClientSession {
            medium: ClientMedium::Wired,
            band: None,
            ssid: None,
            node: None,
            port: None,
            ..session(2, 50, None)
        };
        store
            .save_sessions(vec![roamed.clone(), wired.clone()])
            .await
            .unwrap();
        assert_eq!(
            store.client_sessions(mac(1), ms(0)).await.unwrap(),
            [roamed, session(1, 300, None)]
        );
        assert_eq!(
            store.client_sessions(mac(1), ms(250)).await.unwrap(),
            [session(1, 300, None)]
        );
        assert_eq!(
            store.latest_sessions().await.unwrap(),
            [session(1, 300, None), wired]
        );
    }

    #[tokio::test]
    async fn pruning_forgets_old_sessions_and_unnamed_clients() {
        let (_dir, store, _) = testing::open().await;
        store
            .save_clients(vec![
                record(1, (0, 100)),
                record(2, (0, 100)),
                record(3, (0, 900)),
            ])
            .await
            .unwrap();
        store
            .save_alias(ClientRecord {
                alias: Some("Kept".to_owned()),
                ..record(2, (0, 100))
            })
            .await
            .unwrap();
        store
            .save_sessions(vec![
                session(1, 0, None),
                session(2, 0, Some(100)),
                session(3, 0, Some(100)),
                session(3, 800, Some(900)),
            ])
            .await
            .unwrap();
        assert_eq!(store.prune_clients(ms(500)).await.unwrap(), (1, 3));
        let left: Vec<_> = store
            .clients()
            .await
            .unwrap()
            .into_iter()
            .map(|client| client.mac)
            .collect();
        assert_eq!(left, [mac(2), mac(3)]);
        assert_eq!(
            store.latest_sessions().await.unwrap(),
            [session(3, 800, Some(900))]
        );
    }
}
