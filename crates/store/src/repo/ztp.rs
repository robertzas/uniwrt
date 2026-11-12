//! [`ZtpRepo`] on SQLite: the `ztp_pending` and `ztp_approvals` tables (features/adoption.md,
//! Path B; D-102).

use std::future::Future;

use rusqlite::{OptionalExtension as _, Row, params};
use uniwrt_core::{JobId, MacAddr, Timestamp};
use uniwrt_model::ztp::{PendingDevice, ZtpApproval};

use crate::{
    Result, Store,
    codec::{enum_text, mac_column, timestamp_column},
};

/// The pending adoption inbox and the pre-approvals by MAC.
pub trait ZtpRepo: Send + Sync {
    /// Records a device's hello: a new row, or the latest hello of one in the inbox (its first
    /// sighting and job kept). Then keeps the `keep` most recently seen, so a flood of hellos
    /// can't grow the inbox. Returns the row as stored.
    fn ztp_hello(
        &self,
        device: PendingDevice,
        keep: usize,
    ) -> impl Future<Output = Result<PendingDevice>> + Send;

    /// The inbox, the latest hello first.
    fn ztp_pending(&self) -> impl Future<Output = Result<Vec<PendingDevice>>> + Send;

    fn ztp_pending_device(
        &self,
        mac: MacAddr,
    ) -> impl Future<Output = Result<Option<PendingDevice>>> + Send;

    /// Notes the adoption job started for the device; whether it is in the inbox.
    fn set_ztp_job(&self, mac: MacAddr, job: JobId) -> impl Future<Output = Result<bool>> + Send;

    /// Takes the device out of the inbox; whether it was there.
    fn forget_ztp_pending(&self, mac: MacAddr) -> impl Future<Output = Result<bool>> + Send;

    /// Approves a MAC, replacing an earlier approval of it.
    fn put_ztp_approval(&self, approval: ZtpApproval) -> impl Future<Output = Result<()>> + Send;

    /// The approvals not expired at `now`, by MAC.
    fn ztp_approvals(
        &self,
        now: Timestamp,
    ) -> impl Future<Output = Result<Vec<ZtpApproval>>> + Send;

    /// Withdraws a MAC's approval; whether it had one (expired or not).
    fn delete_ztp_approval(&self, mac: MacAddr) -> impl Future<Output = Result<bool>> + Send;

    /// Uses up a MAC's approval: removes it, and returns it unless it expired by `now`.
    fn take_ztp_approval(
        &self,
        mac: MacAddr,
        now: Timestamp,
    ) -> impl Future<Output = Result<Option<ZtpApproval>>> + Send;
}

const PENDING_COLUMNS: &str = "mac, board_name, model, hostname, release, role, address, \
     host_key, fingerprint, first_seen, last_seen, job_id";

fn pending(row: &Row<'_>) -> rusqlite::Result<PendingDevice> {
    Ok(PendingDevice {
        mac: mac_column(row, 0)?,
        board_name: row.get(1)?,
        model: row.get(2)?,
        hostname: row.get(3)?,
        release: row.get(4)?,
        role: optional_enum(row, 5)?,
        address: row.get::<_, String>(6)?.parse().map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(err))
        })?,
        host_key: row.get(7)?,
        fingerprint: row.get(8)?,
        first_seen: timestamp_column(row, 9)?,
        last_seen: timestamp_column(row, 10)?,
        job: row.get::<_, Option<i64>>(11)?.map(JobId::new),
    })
}

fn approval(row: &Row<'_>) -> rusqlite::Result<ZtpApproval> {
    Ok(ZtpApproval {
        mac: mac_column(row, 0)?,
        name: row.get(1)?,
        role: optional_enum(row, 2)?,
        created_at: timestamp_column(row, 3)?,
        expires_at: timestamp_column(row, 4)?,
        created_by: row.get(5)?,
    })
}

fn optional_enum<T: serde::de::DeserializeOwned>(
    row: &Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<T>> {
    match row.get_ref(index)? {
        rusqlite::types::ValueRef::Null => Ok(None),
        _ => crate::codec::enum_column(row, index).map(Some),
    }
}

impl ZtpRepo for Store {
    async fn ztp_hello(&self, device: PendingDevice, keep: usize) -> Result<PendingDevice> {
        let keep = i64::try_from(keep).unwrap_or(i64::MAX);
        self.write(move |conn| {
            let stored = conn
                .prepare_cached(&format!(
                    "INSERT INTO ztp_pending ({PENDING_COLUMNS})
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, NULL)
                     ON CONFLICT (mac) DO UPDATE SET board_name = excluded.board_name,
                         model = excluded.model, hostname = excluded.hostname,
                         release = excluded.release, role = excluded.role,
                         address = excluded.address, host_key = excluded.host_key,
                         fingerprint = excluded.fingerprint, last_seen = excluded.last_seen
                     RETURNING {PENDING_COLUMNS}"
                ))?
                .query_row(
                    params![
                        device.mac.to_string(),
                        device.board_name,
                        device.model,
                        device.hostname,
                        device.release,
                        device.role.map(enum_text),
                        device.address.to_string(),
                        device.host_key,
                        device.fingerprint,
                        device.last_seen.as_unix_ms(),
                    ],
                    pending,
                )?;
            conn.prepare_cached(
                "DELETE FROM ztp_pending WHERE mac NOT IN
                 (SELECT mac FROM ztp_pending ORDER BY last_seen DESC, mac LIMIT ?1)",
            )?
            .execute(params![keep])?;
            Ok(stored)
        })
        .await
    }

    async fn ztp_pending(&self) -> Result<Vec<PendingDevice>> {
        self.read(|conn| {
            Ok(conn
                .prepare_cached(&format!(
                    "SELECT {PENDING_COLUMNS} FROM ztp_pending ORDER BY last_seen DESC, mac"
                ))?
                .query_map([], pending)?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn ztp_pending_device(&self, mac: MacAddr) -> Result<Option<PendingDevice>> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(&format!(
                    "SELECT {PENDING_COLUMNS} FROM ztp_pending WHERE mac = ?1"
                ))?
                .query_row(params![mac.to_string()], pending)
                .optional()?)
        })
        .await
    }

    async fn set_ztp_job(&self, mac: MacAddr, job: JobId) -> Result<bool> {
        self.write(move |conn| {
            let changed = conn
                .prepare_cached("UPDATE ztp_pending SET job_id = ?2 WHERE mac = ?1")?
                .execute(params![mac.to_string(), job.get()])?;
            Ok(changed > 0)
        })
        .await
    }

    async fn forget_ztp_pending(&self, mac: MacAddr) -> Result<bool> {
        self.write(move |conn| {
            let removed = conn
                .prepare_cached("DELETE FROM ztp_pending WHERE mac = ?1")?
                .execute(params![mac.to_string()])?;
            Ok(removed > 0)
        })
        .await
    }

    async fn put_ztp_approval(&self, approval: ZtpApproval) -> Result<()> {
        self.write(move |conn| {
            conn.prepare_cached(
                "INSERT OR REPLACE INTO ztp_approvals
                 (mac, name, role, created_at, expires_at, created_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?
            .execute(params![
                approval.mac.to_string(),
                approval.name,
                approval.role.map(enum_text),
                approval.created_at.as_unix_ms(),
                approval.expires_at.as_unix_ms(),
                approval.created_by,
            ])?;
            Ok(())
        })
        .await
    }

    async fn ztp_approvals(&self, now: Timestamp) -> Result<Vec<ZtpApproval>> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT mac, name, role, created_at, expires_at, created_by
                     FROM ztp_approvals WHERE expires_at > ?1 ORDER BY mac",
                )?
                .query_map(params![now.as_unix_ms()], approval)?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn delete_ztp_approval(&self, mac: MacAddr) -> Result<bool> {
        self.write(move |conn| {
            let removed = conn
                .prepare_cached("DELETE FROM ztp_approvals WHERE mac = ?1")?
                .execute(params![mac.to_string()])?;
            Ok(removed > 0)
        })
        .await
    }

    async fn take_ztp_approval(&self, mac: MacAddr, now: Timestamp) -> Result<Option<ZtpApproval>> {
        self.write(move |conn| {
            let taken = conn
                .prepare_cached(
                    "DELETE FROM ztp_approvals WHERE mac = ?1
                     RETURNING mac, name, role, created_at, expires_at, created_by",
                )?
                .query_row(params![mac.to_string()], approval)
                .optional()?;
            Ok(taken.filter(|approval| approval.expires_at > now))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uniwrt_core::{JobId, MacAddr, Timestamp};
    use uniwrt_model::{
        node::NodeRole,
        ztp::{PendingDevice, ZtpApproval},
    };

    use super::ZtpRepo as _;
    use crate::testing;

    fn mac(last: u8) -> MacAddr {
        MacAddr::new([0x94, 0x83, 0xc4, 0, 0, last])
    }

    fn at(secs: u64) -> Timestamp {
        testing::START.saturating_add(Duration::from_secs(secs))
    }

    fn device(last: u8, seen: u64) -> PendingDevice {
        PendingDevice {
            mac: mac(last),
            board_name: "glinet,gl-mt6000".to_owned(),
            model: "GL.iNet GL-MT6000".to_owned(),
            hostname: format!("uniwrt-00{last:02x}"),
            release: "24.10.4".to_owned(),
            role: Some(NodeRole::Ap),
            address: format!("10.0.0.{last}").parse().unwrap(),
            host_key: "ssh-ed25519 AAAA".to_owned(),
            fingerprint: "SHA256:x".to_owned(),
            first_seen: at(seen),
            last_seen: at(seen),
            job: None,
        }
    }

    #[tokio::test]
    async fn a_hello_refreshes_its_row_and_the_inbox_stays_bounded() {
        let (_dir, store, _) = testing::open().await;
        store.ztp_hello(device(1, 0), 2).await.unwrap();
        assert!(store.set_ztp_job(mac(1), JobId::new(4)).await.unwrap());
        // Again from another address: its first sighting and job stay.
        let again = PendingDevice {
            address: "10.0.0.9".parse().unwrap(),
            role: None,
            ..device(1, 30)
        };
        let stored = store.ztp_hello(again, 2).await.unwrap();
        assert_eq!(stored.first_seen, at(0));
        assert_eq!(stored.last_seen, at(30));
        assert_eq!(stored.job, Some(JobId::new(4)));
        assert_eq!(stored.role, None);
        assert_eq!(stored.address.to_string(), "10.0.0.9");

        store.ztp_hello(device(2, 10), 2).await.unwrap();
        store.ztp_hello(device(3, 20), 2).await.unwrap();
        // The least recently seen goes.
        let macs: Vec<_> = store
            .ztp_pending()
            .await
            .unwrap()
            .into_iter()
            .map(|device| device.mac)
            .collect();
        assert_eq!(macs, [mac(1), mac(3)]);
        assert_eq!(
            store.ztp_pending_device(mac(3)).await.unwrap(),
            Some(device(3, 20))
        );
        assert!(store.forget_ztp_pending(mac(3)).await.unwrap());
        assert!(!store.forget_ztp_pending(mac(3)).await.unwrap());
        assert!(!store.set_ztp_job(mac(3), JobId::new(5)).await.unwrap());
    }

    #[tokio::test]
    async fn an_approval_is_used_once_and_expires() {
        let (_dir, store, _) = testing::open().await;
        let approval = |last, expires| ZtpApproval {
            mac: mac(last),
            name: Some("Attic".to_owned()),
            role: Some(NodeRole::Bridge),
            created_at: at(0),
            expires_at: at(expires),
            created_by: Some("admin".to_owned()),
        };
        store.put_ztp_approval(approval(1, 100)).await.unwrap();
        store.put_ztp_approval(approval(2, 50)).await.unwrap();
        assert_eq!(
            store.ztp_approvals(at(60)).await.unwrap(),
            [approval(1, 100)]
        );
        // Expired: removed, not given.
        assert_eq!(store.take_ztp_approval(mac(2), at(60)).await.unwrap(), None);
        assert_eq!(
            store.ztp_approvals(at(0)).await.unwrap(),
            [approval(1, 100)]
        );
        // Used once.
        assert_eq!(
            store.take_ztp_approval(mac(1), at(60)).await.unwrap(),
            Some(approval(1, 100))
        );
        assert_eq!(store.take_ztp_approval(mac(1), at(60)).await.unwrap(), None);
        store.put_ztp_approval(approval(3, 100)).await.unwrap();
        assert!(store.delete_ztp_approval(mac(3)).await.unwrap());
        assert!(!store.delete_ztp_approval(mac(3)).await.unwrap());
    }
}
