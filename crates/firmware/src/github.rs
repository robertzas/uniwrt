//! GitHub releases as a source of upgrade images (D-121, D-122): a repo's releases, read from
//! GitHub's REST API (with a token for private repos and the higher rate limit) and kept ten
//! minutes; per node, the newest five with an image for it ([`Releases::offers`]), each image
//! with the SHA-256 GitHub records for it.

mod fit;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use hyper::header::{AUTHORIZATION, HeaderName, HeaderValue};
use serde::Deserialize;
use uniwrt_core::Timestamp;
use uniwrt_model::firmware::{FirmwareImage, GitHubRelease};

pub use fit::Device;

use crate::{Error, ImageCache, http::Http, valid_sha256};

/// How long a repo's releases are kept: GitHub allows 60 requests an hour without a token.
const TTL: Duration = Duration::from_mins(10);
/// The repos kept.
const MAX_REPOS: usize = 8;
/// The releases read (one page, newest first).
const PER_PAGE: usize = 30;
/// The releases offered per node: the newest, to pin or roll back (D-122).
pub const MAX_OFFERS: usize = 5;
/// The largest answer read: a page of releases with many assets each.
const MAX_ANSWER: usize = 8 << 20;

/// GitHub's REST API. Clones share the releases kept.
#[derive(Clone)]
pub struct GitHub {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) base: String,
    pub(crate) http: Http,
    cache: Mutex<Vec<Kept>>,
}

/// A repo's releases kept: the repo, whether a token read them, and when.
type Kept = (String, bool, Instant, Arc<Releases>);

/// A repo's releases, newest first, drafts left out.
#[derive(Debug)]
pub struct Releases {
    repo: String,
    pub(crate) list: Vec<Release>,
}

#[derive(Debug)]
pub(crate) struct Release {
    pub(crate) tag: String,
    pub(crate) name: String,
    pub(crate) prerelease: bool,
    pub(crate) published_at: Option<Timestamp>,
    pub(crate) notes: String,
    pub(crate) assets: Vec<Asset>,
}

#[derive(Debug)]
pub(crate) struct Asset {
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) sha256: Option<String>,
    pub(crate) url: String,
    /// The asset in GitHub's API, where a token downloads it (a private repo's).
    pub(crate) api_url: Option<String>,
}

impl GitHub {
    pub const PUBLIC: &'static str = "https://api.github.com";

    /// GitHub's API at `base`, which uniwrt calls `client` (`uniwrt/<version>`).
    pub fn new(base: &str, client: &str) -> Result<Self, Error> {
        let base = base.trim_end_matches('/');
        if !(base.starts_with("https://") || base.starts_with("http://")) {
            return Err(Error::Invalid(format!("{base} is not an HTTP(S) URL")));
        }
        Ok(Self {
            inner: Arc::new(Inner {
                base: base.to_owned(),
                http: Http::new(client)?,
                cache: Mutex::default(),
            }),
        })
    }

    /// `repo`'s releases ([`parse_repo`]'s form), read with `token` when one is kept.
    pub async fn releases(&self, repo: &str, token: Option<&str>) -> Result<Arc<Releases>, Error> {
        if !valid_repo(repo) {
            return Err(Error::Invalid(format!(
                "{repo} isn't a GitHub repo (owner/name)"
            )));
        }
        let now = Instant::now();
        let with_token = token.is_some();
        let cached = self
            .cache()
            .iter()
            .find_map(|(known, token, at, releases)| {
                (known == repo && *token == with_token && now.duration_since(*at) < TTL)
                    .then(|| Arc::clone(releases))
            });
        if let Some(releases) = cached {
            return Ok(releases);
        }
        let headers = api_headers(token)?;
        let url = format!(
            "{}/repos/{repo}/releases?per_page={PER_PAGE}",
            self.inner.base
        );
        let answer = self.inner.http.get_with(&url, &headers, MAX_ANSWER).await?;
        let releases = match answer.status {
            200 => Arc::new(parse(repo, &answer.body)?),
            status => return Err(refusal(repo, with_token, status, &answer)),
        };
        let mut cache = self.cache();
        cache.retain(|(known, token, _, _)| !(known == repo && *token == with_token));
        cache.push((repo.to_owned(), with_token, now, Arc::clone(&releases)));
        if cache.len() > MAX_REPOS {
            cache.remove(0);
        }
        Ok(releases)
    }

    /// Forgets the releases kept: the token or a repo changed.
    /// The asset `name` of `repo`'s release `tag`, downloaded into `cache` unless kept there
    /// (D-124): with a token through GitHub's API, where a private repo's assets download (the
    /// token isn't sent on to the storage it redirects to), else from its download URL; checked
    /// against the SHA-256 GitHub lists.
    pub async fn fetch_image(
        &self,
        cache: &ImageCache,
        repo: &str,
        tag: &str,
        name: &str,
        token: Option<&str>,
    ) -> Result<PathBuf, Error> {
        let releases = self.releases(repo, token).await?;
        let asset = releases
            .list
            .iter()
            .find(|release| release.tag == tag)
            .and_then(|release| release.assets.iter().find(|asset| asset.name == name))
            .ok_or_else(|| Error::NotFound(format!("{repo}'s release {tag} has no {name}")))?;
        let image = FirmwareImage {
            name: asset.name.clone(),
            kind: "sysupgrade".to_owned(),
            filesystem: None,
            size_bytes: Some(asset.size),
            sha256: asset
                .sha256
                .clone()
                .ok_or_else(|| Error::Untrusted(format!("GitHub lists no SHA-256 of {name}")))?,
            url: asset.url.clone(),
        };
        match (token, &asset.api_url) {
            (Some(_), Some(api)) => {
                let headers = api_headers(token)?;
                cache.fetch_from(&image, api, &headers).await
            }
            _ => cache.fetch(&image).await,
        }
    }

    pub fn forget(&self) {
        self.cache().clear();
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, Vec<Kept>> {
        self.inner
            .cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl Releases {
    pub fn repo(&self) -> &str {
        &self.repo
    }

    /// The newest [`MAX_OFFERS`] releases with an image for `device` (prereleases only when
    /// asked), and why there are none, when there are none.
    pub fn offers(
        &self,
        device: Device<'_>,
        prereleases: bool,
    ) -> (Vec<GitHubRelease>, Option<String>) {
        let mut unhashed = false;
        let mut found = Vec::new();
        for release in &self.list {
            if release.prerelease && !prereleases {
                continue;
            }
            let mut images = Vec::new();
            for asset in &release.assets {
                let Some((kind, filesystem)) = fit::fits(&asset.name, &device) else {
                    continue;
                };
                let Some(sha256) = &asset.sha256 else {
                    unhashed = true;
                    continue;
                };
                images.push(FirmwareImage {
                    name: asset.name.clone(),
                    kind,
                    filesystem,
                    size_bytes: Some(asset.size),
                    sha256: sha256.clone(),
                    url: asset.url.clone(),
                });
            }
            if images.is_empty() {
                continue;
            }
            images.sort_by(|a, b| {
                let rank = |image: &FirmwareImage| image.filesystem.as_deref() != Some("squashfs");
                rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name))
            });
            found.push(GitHubRelease {
                tag: release.tag.clone(),
                name: release.name.clone(),
                prerelease: release.prerelease,
                published_at: release.published_at,
                notes: release.notes.clone(),
                images,
            });
            if found.len() == MAX_OFFERS {
                break;
            }
        }
        let note = found.is_empty().then(|| {
            if unhashed {
                format!(
                    "The images of {} for {} have no SHA-256 on GitHub, which uniwrt checks them \
                     against: upload them to the release again",
                    self.repo, device.board
                )
            } else {
                format!(
                    "No release of {} has a sysupgrade image named for {}",
                    self.repo,
                    device.profile.unwrap_or(device.board)
                )
            }
        });
        (found, note)
    }
}

/// A repo as typed: `owner/name`, or its URL on GitHub; `None` when it is neither.
pub fn parse_repo(text: &str) -> Option<String> {
    let text = text.trim();
    let path = text
        .strip_prefix("https://github.com/")
        .or_else(|| text.strip_prefix("http://github.com/"))
        .or_else(|| text.strip_prefix("github.com/"))
        .unwrap_or(text);
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    let repo = format!("{owner}/{name}");
    (valid_repo(&repo) && matches!(parts.next(), None | Some("releases"))).then_some(repo)
}

/// `owner/name` as GitHub allows them.
pub fn valid_repo(repo: &str) -> bool {
    let Some((owner, name)) = repo.split_once('/') else {
        return false;
    };
    let owner_ok = (1..=39).contains(&owner.len())
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !owner.starts_with('-');
    let name_ok = (1..=100).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && name != "."
        && name != "..";
    owner_ok && name_ok
}

/// The headers of a request to GitHub's API: its version, and the token when one is kept.
pub(crate) fn api_headers(token: Option<&str>) -> Result<Vec<(HeaderName, HeaderValue)>, Error> {
    let mut headers = vec![(
        HeaderName::from_static("x-github-api-version"),
        HeaderValue::from_static("2022-11-28"),
    )];
    if let Some(token) = token {
        let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| Error::GitHub("The GitHub token isn't valid".to_owned()))?;
        value.set_sensitive(true);
        headers.push((AUTHORIZATION, value));
    }
    Ok(headers)
}

/// A token as GitHub issues them: printable ASCII without spaces.
pub fn valid_token(token: &str) -> bool {
    (1..=255).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_graphic())
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: Option<String>,
    html_url: String,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    size: u64,
    #[serde(default)]
    digest: Option<String>,
    browser_download_url: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

fn parse(repo: &str, body: &[u8]) -> Result<Releases, Error> {
    let raw: Vec<RawRelease> = serde_json::from_slice(body).map_err(|err| {
        Error::Invalid(format!("GitHub's releases of {repo} are unreadable: {err}"))
    })?;
    let mut list: Vec<Release> = raw
        .into_iter()
        .filter(|release| {
            !release.draft && printable(&release.tag_name) && web_url(&release.html_url)
        })
        .map(|release| Release {
            name: release
                .name
                .filter(|name| printable(name))
                .unwrap_or_else(|| release.tag_name.clone()),
            tag: release.tag_name,
            prerelease: release.prerelease,
            published_at: release
                .published_at
                .as_deref()
                .and_then(Timestamp::parse_utc),
            notes: release.html_url,
            assets: release
                .assets
                .into_iter()
                .filter(|asset| {
                    asset
                        .state
                        .as_deref()
                        .is_none_or(|state| state == "uploaded")
                        && printable(&asset.name)
                        && web_url(&asset.browser_download_url)
                })
                .map(|asset| Asset {
                    sha256: asset
                        .digest
                        .as_deref()
                        .and_then(|digest| digest.strip_prefix("sha256:"))
                        .filter(|hex| valid_sha256(hex))
                        .map(str::to_owned),
                    name: asset.name,
                    size: asset.size,
                    url: asset.browser_download_url,
                    api_url: asset.url.filter(|url| web_url(url)),
                })
                .collect(),
        })
        .collect();
    // Newest first: GitHub lists by creation, and a release may be published later.
    list.sort_by_key(|release| std::cmp::Reverse(release.published_at));
    Ok(Releases {
        repo: repo.to_owned(),
        list,
    })
}

/// What GitHub's refusal means, in its words where it gave some.
pub(crate) fn refusal(
    repo: &str,
    with_token: bool,
    status: u16,
    answer: &crate::http::Answer,
) -> Error {
    let header = |name: &str| {
        answer
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let spent = header("x-ratelimit-remaining").as_deref() == Some("0");
    match status {
        403 | 429 if spent => {
            let reset = header("x-ratelimit-reset")
                .and_then(|secs| secs.parse::<i64>().ok())
                .map_or_default(|secs| format!(" until {}", Timestamp::from_unix_secs(secs)));
            let hint = if with_token {
                ""
            } else {
                "; a token raises it"
            };
            Error::GitHub(format!("GitHub's rate limit is spent{reset}{hint}"))
        }
        401 => Error::Unauthorized(
            "GitHub refused the token: it expired or was revoked. Set a new one".to_owned(),
        ),
        404 => Error::NotFound(if with_token {
            format!("GitHub has no repo {repo} the token can read")
        } else {
            format!("GitHub has no public repo {repo} (a private one needs a token)")
        }),
        status => {
            let message = serde_json::from_slice::<serde_json::Value>(&answer.body)
                .ok()
                .and_then(|value| value["message"].as_str().map(str::to_owned))
                .filter(|text| printable(text))
                .map_or_default(|text| format!(": {text}"));
            Error::GitHub(format!("GitHub answered {status} for {repo}{message}"))
        }
    }
}

fn printable(text: &str) -> bool {
    !text.is_empty() && text.len() <= 255 && !text.chars().any(char::is_control)
}

fn web_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://")) && printable(url)
}

#[cfg(test)]
#[path = "github_tests.rs"]
mod tests;
