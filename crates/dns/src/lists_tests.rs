use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use uniwrt_core::Timestamp;
use uniwrt_model::site::blocking::{Blocking, CustomList};

use super::{Blocklists, Wanted, wanted};
use crate::{
    Error,
    http::{BoxFuture, Fetched, Source, Validators},
};

/// A URL's scripted answer: a body and its `ETag`, or why it fails.
type Answer = Result<(String, String), String>;

/// Answers from a script: a body with an `ETag`, 304 for a matching `ETag`, or an error.
#[derive(Default)]
struct Script {
    answers: Mutex<BTreeMap<String, Answer>>,
    asked: Mutex<Vec<(String, Validators)>>,
}

impl Script {
    fn set(&self, url: &str, answer: Result<(&str, &str), &str>) {
        let answer = answer
            .map(|(body, etag)| (body.to_owned(), etag.to_owned()))
            .map_err(str::to_owned);
        self.answers.lock().unwrap().insert(url.to_owned(), answer);
    }
}

impl Source for Script {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        validators: &'a Validators,
    ) -> BoxFuture<'a, Result<Fetched, Error>> {
        self.asked
            .lock()
            .unwrap()
            .push((url.to_owned(), validators.clone()));
        let answer = self.answers.lock().unwrap().get(url).cloned();
        Box::pin(async move {
            match answer {
                Some(Ok((_, etag))) if validators.etag.as_deref() == Some(etag.as_str()) => {
                    Ok(Fetched::NotModified)
                }
                Some(Ok((body, etag))) => Ok(Fetched::Body {
                    body: Bytes::from(body),
                    validators: Validators {
                        etag: Some(etag),
                        last_modified: None,
                    },
                }),
                Some(Err(why)) => Err(Error::Unreachable {
                    url: url.to_owned(),
                    why,
                }),
                None => Err(Error::Status {
                    url: url.to_owned(),
                    status: 404,
                }),
            }
        })
    }
}

fn list(url: &str) -> Wanted {
    Wanted {
        id: None,
        name: url.to_owned(),
        url: url.to_owned(),
    }
}

fn at(s: i64) -> Timestamp {
    Timestamp::from_unix_secs(s)
}

#[test]
fn wanted_puts_malware_first_then_custom_then_ads_once_each() {
    let blocking = Blocking {
        enabled: true,
        lists: vec!["hagezi_normal".into(), "urlhaus".into(), "nonsense".into()],
        custom: vec![
            CustomList {
                name: "Mine".into(),
                url: "https://example.com/mine.txt".into(),
            },
            CustomList {
                name: "Again".into(),
                url: "https://example.com/mine.txt".into(),
            },
        ],
        ..Blocking::default()
    };
    let names: Vec<_> = wanted(&blocking).into_iter().map(|w| w.name).collect();
    assert_eq!(names, ["URLhaus", "Mine", "HaGeZi Normal"]);
}

#[tokio::test]
async fn refreshes_keep_the_last_good_copy_and_ask_conditionally() {
    let dir = tempfile::tempdir().unwrap();
    let script = Arc::new(Script::default());
    script.set("a", Ok(("one.example.com\ntwo.example.com\n", "e1")));
    script.set("b", Ok(("0.0.0.0 three.example.com\n", "e2")));
    let lists = Blocklists::open(dir.path().to_owned(), script.clone())
        .await
        .unwrap();
    let wanted = [list("a"), list("b")];
    assert_eq!(lists.attempted(&wanted), None);
    assert!(lists.refresh(&wanted, at(100)).await);
    let status = lists.status(&wanted);
    assert_eq!(status[0].domains, Some(2));
    assert_eq!(status[1].fetched_at, Some(at(100)));
    let blocking = Blocking::default();
    let compiled = lists.compile(&wanted, &blocking, u32::MAX).await.unwrap();
    assert_eq!(compiled.domains, 3);

    // Unchanged, then failing: the copy stays, the error shows, nothing changed.
    script.set("b", Err("offline"));
    assert!(!lists.refresh(&wanted, at(200)).await);
    let asked = script.asked.lock().unwrap().clone();
    let second = asked[2..].iter().find(|(url, _)| url == "a").unwrap();
    assert_eq!(second.1.etag.as_deref(), Some("e1"));
    let status = lists.status(&wanted);
    assert_eq!(
        (status[0].checked_at, status[0].fetched_at),
        (Some(at(200)), Some(at(100)))
    );
    assert_eq!(status[1].error.as_deref(), Some("b: offline"));
    assert_eq!(status[1].checked_at, Some(at(100)));
    assert_eq!(lists.attempted(&wanted), Some(at(200)));
    let again = lists.compile(&wanted, &blocking, u32::MAX).await.unwrap();
    assert_eq!(again, compiled);

    // Reopened, the index is read back; a list dropped loses its copy.
    let lists = Blocklists::open(dir.path().to_owned(), script.clone())
        .await
        .unwrap();
    assert_eq!(lists.status(&wanted)[0].domains, Some(2));
    lists.refresh(&wanted[..1], at(300)).await;
    assert_eq!(lists.status(&wanted)[1].domains, None);
    let left = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(left, 2, "the index and one copy");
}
