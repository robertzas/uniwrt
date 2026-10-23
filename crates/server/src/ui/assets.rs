//! The asset table: one entry per UI file, each with optional brotli and gzip variants.

/// One stored representation of an asset.
#[derive(Debug)]
pub(crate) struct Variant {
    /// A strong, quoted `ETag`, distinct for each representation.
    pub(crate) etag: &'static str,
    pub(crate) bytes: &'static [u8],
}

/// A UI file. Compressed variants exist only where they save at least 10%.
#[derive(Debug)]
pub(crate) struct Asset {
    /// Relative to the UI root, without a leading slash.
    pub(crate) path: &'static str,
    pub(crate) content_type: &'static str,
    pub(crate) identity: Variant,
    pub(crate) brotli: Option<Variant>,
    pub(crate) gzip: Option<Variant>,
}

/// Where the embedded UI came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "the generated table constructs only the variant this build embeds"
)]
pub(crate) enum Source {
    /// The real UI (`web/dist` or `UNIWRT_WEB_DIST`).
    WebDist,
    /// `crates/server/placeholder`, for builds without the web UI.
    Placeholder,
}

#[derive(Debug)]
pub(crate) struct Assets {
    pub(crate) source: Source,
    /// Directory of content-hashed files: cached forever, and a miss there is a 404, never the
    /// SPA page (a stale chunk must not turn into HTML).
    pub(crate) hashed_dir: &'static str,
    /// Sorted by `path`, without duplicates.
    pub(crate) files: &'static [Asset],
    /// Position of `index.html` in `files`.
    pub(crate) index: usize,
}

impl Assets {
    pub(crate) fn get(&self, path: &str) -> Option<&Asset> {
        let found = self.files.binary_search_by(|asset| asset.path.cmp(path));
        found.ok().map(|n| &self.files[n])
    }

    pub(crate) fn index(&self) -> &Asset {
        &self.files[self.index]
    }

    pub(crate) fn is_hashed(&self, path: &str) -> bool {
        path.starts_with(self.hashed_dir)
    }
}

include!(concat!(env!("OUT_DIR"), "/ui.rs"));

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use axum::http::HeaderValue;

    use super::EMBEDDED;

    #[test]
    fn embedded_table_is_sorted_and_has_an_index() {
        assert!(
            EMBEDDED
                .files
                .windows(2)
                .all(|pair| pair[0].path < pair[1].path)
        );
        assert_eq!(EMBEDDED.index().path, "index.html");
        assert!(
            EMBEDDED
                .files
                .iter()
                .all(|asset| EMBEDDED.get(asset.path).is_some())
        );
    }

    #[test]
    fn embedded_variants_are_valid_and_decode_to_the_original() {
        for asset in EMBEDDED.files {
            HeaderValue::from_str(asset.content_type).unwrap();
            let original = asset.identity.bytes;
            let mut etags = vec![asset.identity.etag];
            if let Some(br) = &asset.brotli {
                let mut decoded = Vec::new();
                brotli::Decompressor::new(br.bytes, 4096)
                    .read_to_end(&mut decoded)
                    .unwrap();
                assert_eq!(decoded, original, "{} (br)", asset.path);
                assert!(br.bytes.len() < original.len());
                etags.push(br.etag);
            }
            if let Some(gz) = &asset.gzip {
                let mut decoded = Vec::new();
                flate2::read::GzDecoder::new(gz.bytes)
                    .read_to_end(&mut decoded)
                    .unwrap();
                assert_eq!(decoded, original, "{} (gzip)", asset.path);
                assert!(gz.bytes.len() < original.len());
                etags.push(gz.etag);
            }
            for etag in &etags {
                HeaderValue::from_str(etag).unwrap();
                assert!(
                    etag.len() > 2 && etag.starts_with('"') && etag.ends_with('"'),
                    "{etag}"
                );
            }
            etags.sort_unstable();
            etags.dedup();
            assert_eq!(
                etags.len(),
                1 + usize::from(asset.brotli.is_some()) + usize::from(asset.gzip.is_some())
            );
        }
    }
}
