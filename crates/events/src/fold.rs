//! Folding repeats: an event with a fold key that happens again within the window of the
//! last one (from its latest repeat) becomes a repeat of that record (its count grows and its
//! `last_at` moves) instead of a new one, so a device retrying a wrong password makes one line.

use std::{collections::HashMap, time::Duration};

use uniwrt_core::{EventId, Timestamp};
use uniwrt_model::events::{EventWrite as Write, NewEvent};

#[derive(Debug)]
pub struct Folder {
    window: Duration,
    /// By fold key: the record and its latest repeat.
    open: HashMap<String, (EventId, Timestamp)>,
}

impl Folder {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            open: HashMap::new(),
        }
    }

    /// What to store for `events`: repeats within the batch merge first, then repeats of open
    /// records fold into them. Inserts with a fold key go to [`Folder::opened`] once stored.
    pub fn plan(&mut self, events: Vec<NewEvent>) -> Vec<Write> {
        let mut writes: Vec<Write> = Vec::with_capacity(events.len());
        // By fold key: the index in `writes` of the batch's latest write.
        let mut latest = HashMap::<String, usize>::new();
        for event in events {
            let Some(key) = event.fold.clone() else {
                writes.push(Write::Insert(event));
                continue;
            };
            let at = event.at;
            if let Some(&index) = latest.get(&key) {
                let within = |last: Timestamp| at.saturating_duration_since(last) <= self.window;
                match &mut writes[index] {
                    Write::Insert(first) if within(first.last_at) => {
                        first.last_at = first.last_at.max(at);
                        first.count += 1;
                        continue;
                    }
                    Write::Fold { last_at, count, .. } if within(*last_at) => {
                        *last_at = (*last_at).max(at);
                        *count += 1;
                        continue;
                    }
                    _ => {}
                }
            } else if let Some(&(id, last)) = self.open.get(&key)
                && at.saturating_duration_since(last) <= self.window
            {
                latest.insert(key, writes.len());
                writes.push(Write::Fold {
                    id,
                    last_at: at.max(last),
                    count: 1,
                });
                continue;
            }
            self.open.remove(&key);
            latest.insert(key, writes.len());
            writes.push(Write::Insert(event));
        }
        for write in &writes {
            if let Write::Fold { id, last_at, .. } = write
                && let Some(open) = self.open.values_mut().find(|(open, _)| open == id)
            {
                open.1 = *last_at;
            }
        }
        writes
    }

    /// An insert with fold key `key` was stored as `id`; its latest repeat is `last_at`.
    pub fn opened(&mut self, key: String, id: EventId, last_at: Timestamp) {
        self.open.insert(key, (id, last_at));
    }

    /// Forgets records whose window has passed.
    pub fn expire(&mut self, now: Timestamp) {
        let window = self.window;
        self.open
            .retain(|_, (_, last)| now.saturating_duration_since(*last) <= window);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uniwrt_core::{EventId, LogId, Timestamp};
    use uniwrt_model::events::{EventKind, EventWrite as Write, NewEvent};

    use super::Folder;

    fn wrong_password(s: i64) -> NewEvent {
        NewEvent::new(
            Timestamp::from_unix_ms(s * 1_000),
            EventKind::LogPattern {
                pattern: "wifi-wrong-password".into(),
                log: LogId::new(s),
                host: None,
                program: Some("hostapd".into()),
                text: "AP-STA-POSSIBLE-PSK-MISMATCH".into(),
            },
        )
        .fold("log:wifi-wrong-password:node:1")
    }

    #[test]
    fn repeats_in_a_batch_merge_and_later_ones_fold() {
        let mut folder = Folder::new(Duration::from_mins(5));
        let writes = folder.plan(vec![
            wrong_password(0),
            wrong_password(10),
            wrong_password(20),
        ]);
        let [Write::Insert(first)] = writes.as_slice() else {
            panic!("one insert: {writes:?}");
        };
        assert_eq!((first.count, first.last_at.as_unix_ms()), (3, 20_000));
        folder.opened(first.fold.clone().unwrap(), EventId::new(7), first.last_at);
        let writes = folder.plan(vec![wrong_password(100), wrong_password(110)]);
        assert_eq!(
            writes,
            vec![Write::Fold {
                id: EventId::new(7),
                last_at: Timestamp::from_unix_ms(110_000),
                count: 2
            }]
        );
        // The window runs from the latest repeat.
        let writes = folder.plan(vec![wrong_password(400)]);
        assert!(matches!(writes[..], [Write::Fold { count: 1, .. }]));
        let writes = folder.plan(vec![wrong_password(800)]);
        assert!(matches!(writes[..], [Write::Insert(_)]), "{writes:?}");
    }

    #[test]
    fn events_without_a_key_never_fold() {
        let mut folder = Folder::new(Duration::from_mins(5));
        let event = |s| NewEvent::new(Timestamp::from_unix_ms(s), EventKind::NodeAdopted);
        assert_eq!(folder.plan(vec![event(0), event(1)]).len(), 2);
        folder.opened("k".into(), EventId::new(1), Timestamp::from_unix_ms(0));
        folder.expire(Timestamp::from_unix_ms(301_000));
        assert!(folder.open.is_empty());
    }
}
