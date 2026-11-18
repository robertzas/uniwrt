//! DNS filtering's list copies (D-133): in `<data>/blocklists`, downloaded over HTTP(S); demo
//! mode's come from [`DemoLists`] instead, with no internet.

use std::{path::Path, sync::Arc};

use anyhow::Context as _;
use bytes::Bytes;
use uniwrt_dns::{Blocklists, BoxFuture, Error, Fetched, HttpSource, Source, Validators};
use uniwrt_model::site::blocking::CATALOG;

use crate::build_info;

/// The copies' directory in the data dir.
const DIR: &str = "blocklists";

/// Opens the copies in `state_dir`, downloaded by `demo`'s source or over HTTP(S).
pub(crate) async fn open(state_dir: &Path, demo: bool) -> anyhow::Result<Blocklists> {
    let source: Arc<dyn Source> = if demo {
        Arc::new(DemoLists)
    } else {
        HttpSource::new(&format!("uniwrt/{}", build_info::VERSION))
            .context("cannot set up the lists' downloads")?
    };
    Blocklists::open(state_dir.join(DIR), source)
        .await
        .context("cannot open the blocklists' copies")
}

/// Demo mode's lists: made-up domains, the same for the same URL; a catalog list gets about a
/// hundredth of the domains the catalog gives it (200 to 5,000), a custom one 300. A third of
/// each list's domains are shared by every list, so the compile has duplicates to drop. Asked
/// again, a list is unchanged (its `ETag` matches).
pub(crate) struct DemoLists;

const ETAG: &str = "\"demo\"";

impl DemoLists {
    fn body(url: &str) -> Bytes {
        use std::fmt::Write as _;

        let catalog = CATALOG.iter().find(|list| list.url == url);
        let (slug, n) = catalog.map_or(("custom", 300), |list| {
            (list.id, (list.domains / 100).clamp(200, 5_000))
        });
        let mut body = format!("# {url}, simulated\n");
        for i in 0..n {
            let _ = if i % 3 == 0 {
                writeln!(body, "0.0.0.0 track{i}.shared.example")
            } else {
                writeln!(body, "0.0.0.0 ads{i}.{slug}.example")
            };
        }
        Bytes::from(body)
    }
}

impl Source for DemoLists {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        validators: &'a Validators,
    ) -> BoxFuture<'a, Result<Fetched, Error>> {
        Box::pin(async move {
            if validators.etag.as_deref() == Some(ETAG) {
                return Ok(Fetched::NotModified);
            }
            Ok(Fetched::Body {
                body: Self::body(url),
                validators: Validators {
                    etag: Some(ETAG.to_owned()),
                    last_modified: None,
                },
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_dns::{Fetched, Source as _, Validators};
    use uniwrt_model::site::blocking::catalog_list;

    use super::DemoLists;

    #[tokio::test]
    async fn demo_lists_are_made_up_and_kept() {
        let url = catalog_list("hagezi_normal").unwrap().url;
        let Fetched::Body { body, validators } =
            DemoLists.fetch(url, &Validators::default()).await.unwrap()
        else {
            panic!("a body");
        };
        let lines = body
            .split(|&b| b == b'\n')
            .filter(|l| l.starts_with(b"0.0.0.0"))
            .count();
        assert!((200..=5_000).contains(&lines), "{lines}");
        assert_eq!(
            DemoLists.fetch(url, &validators).await.unwrap(),
            Fetched::NotModified
        );
    }
}
