//! The ASU client: OpenWrt's catalog, kept a while in memory, and zero-touch image builds.

use std::{
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use uniwrt_model::firmware::{FirmwareProfile, FirmwareReleases, ImageBuild, ImageBuildStarted};

use crate::{
    Error,
    build::{self, MAX_DEFAULTS},
    catalog::{self, Overview},
    http::Http,
    packages::packages,
};

/// How long the releases are kept: a new one shows within the hour.
const OVERVIEW_TTL: Duration = Duration::from_hours(1);
/// How long a release's devices are kept: a release's catalog doesn't change.
const PROFILES_TTL: Duration = Duration::from_hours(24);
/// The releases whose devices are kept.
const MAX_RELEASES: usize = 4;
/// The largest catalog read (a release's is about 400 KB).
const MAX_CATALOG: usize = 8 << 20;
/// The largest other answer read.
const MAX_ANSWER: usize = 1 << 20;

/// A zero-touch image to build.
#[derive(Debug, Clone, Copy)]
pub struct ImageRequest<'a> {
    pub release: &'a str,
    pub target: &'a str,
    pub profile: &'a str,
    /// The first-boot script (`/etc/uci-defaults`), without secrets (D-022).
    pub script: &'a str,
}

/// An ASU service and the download server it builds from. Clones share the catalog.
#[derive(Clone)]
pub struct Asu {
    inner: Arc<Inner>,
}

struct Inner {
    base: String,
    client: String,
    http: Http,
    cache: Mutex<Cache>,
}

#[derive(Default)]
struct Cache {
    overview: Option<(Instant, Overview)>,
    profiles: Vec<(String, Instant, Arc<[FirmwareProfile]>)>,
}

impl Asu {
    /// OpenWrt's public ASU.
    pub const PUBLIC: &'static str = "https://sysupgrade.openwrt.org";

    /// The ASU at `base`, which uniwrt calls `client` (`uniwrt/<version>`) in its requests.
    pub fn new(base: &str, client: &str) -> Result<Self, Error> {
        let base = base.trim_end_matches('/');
        if !(base.starts_with("https://") || base.starts_with("http://")) {
            return Err(Error::Invalid(format!("{base} is not an HTTP(S) URL")));
        }
        Ok(Self {
            inner: Arc::new(Inner {
                base: base.to_owned(),
                client: client.to_owned(),
                http: Http::new(client)?,
                cache: Mutex::default(),
            }),
        })
    }

    /// Its URL, without a trailing `/`.
    pub fn url(&self) -> &str {
        &self.inner.base
    }

    /// The releases images can be built for.
    pub async fn releases(&self) -> Result<FirmwareReleases, Error> {
        Ok(FirmwareReleases {
            releases: self.overview().await?.releases,
        })
    }

    /// The devices of `release`, one of [`Self::releases`].
    pub async fn profiles(&self, release: &str) -> Result<Arc<[FirmwareProfile]>, Error> {
        let overview = self.overview().await?;
        if !overview.releases.iter().any(|known| known == release) {
            return Err(Error::NotFound(format!(
                "{release} is not a release images are built for"
            )));
        }
        let now = Instant::now();
        let cached = self
            .cache()
            .profiles
            .iter()
            .find_map(|(known, at, profiles)| {
                (known == release && now.duration_since(*at) < PROFILES_TTL)
                    .then(|| Arc::clone(profiles))
            });
        if let Some(profiles) = cached {
            return Ok(profiles);
        }
        let url = format!("{}/releases/{release}/.overview.json", overview.upstream);
        let body = self.get(&url, MAX_CATALOG).await?;
        let profiles: Arc<[FirmwareProfile]> = catalog::profiles(&body)?.into();
        let mut cache = self.cache();
        cache.profiles.retain(|(known, _, _)| known != release);
        cache
            .profiles
            .push((release.to_owned(), now, Arc::clone(&profiles)));
        if cache.profiles.len() > MAX_RELEASES {
            cache.profiles.remove(0);
        }
        Ok(profiles)
    }

    /// The device `profile` of `target` in `release`.
    pub async fn profile(
        &self,
        release: &str,
        target: &str,
        profile: &str,
    ) -> Result<FirmwareProfile, Error> {
        self.profiles(release)
            .await?
            .iter()
            .find(|known| known.id == profile && known.target == target)
            .cloned()
            .ok_or_else(|| {
                Error::NotFound(format!("{release} has no device {profile} in {target}"))
            })
    }

    /// Asks for an image: the device's defaults with the mesh essentials, and the first-boot
    /// script. ASU answers at once with a build it has, else queues it.
    pub async fn build(&self, image: ImageRequest<'_>) -> Result<ImageBuildStarted, Error> {
        if image.script.len() > MAX_DEFAULTS {
            return Err(Error::Refused(format!(
                "the first-boot script is longer than ASU's {MAX_DEFAULTS} bytes"
            )));
        }
        self.profile(image.release, image.target, image.profile)
            .await?;
        let url = format!(
            "{}/json/v1/releases/{}/targets/{}/{}.json",
            self.inner.base, image.release, image.target, image.profile
        );
        let info = catalog::profile_info(&self.get(&url, MAX_ANSWER).await?)?;
        let packages = packages(&info);
        let request = build::Request {
            version: image.release,
            target: image.target,
            profile: image.profile,
            packages: &packages,
            defaults: image.script,
            client: &self.inner.client,
        };
        let body = serde_json::to_vec(&request).map_err(|err| Error::Invalid(err.to_string()))?;
        let answer = self
            .inner
            .http
            .post_json(
                &format!("{}/api/v1/build", self.inner.base),
                body,
                MAX_ANSWER,
            )
            .await?;
        let build = build::answer(&self.inner.base, None, answer.status, &answer.body)?;
        Ok(ImageBuildStarted { packages, build })
    }

    /// How the build `hash` stands.
    pub async fn status(&self, hash: &str) -> Result<ImageBuild, Error> {
        if !build::valid_hash(hash) {
            return Err(Error::NotFound(format!("{hash} is not a build")));
        }
        let url = format!("{}/api/v1/build/{hash}", self.inner.base);
        let answer = self.inner.http.get(&url, MAX_ANSWER).await?;
        build::answer(&self.inner.base, Some(hash), answer.status, &answer.body)
    }

    async fn overview(&self) -> Result<Overview, Error> {
        let now = Instant::now();
        if let Some((at, overview)) = &self.cache().overview
            && now.duration_since(*at) < OVERVIEW_TTL
        {
            return Ok(overview.clone());
        }
        let url = format!("{}/json/v1/overview.json", self.inner.base);
        let overview = catalog::overview(&self.get(&url, MAX_ANSWER).await?)?;
        self.cache().overview = Some((now, overview.clone()));
        Ok(overview)
    }

    /// A GET that must answer 200.
    async fn get(&self, url: &str, max: usize) -> Result<bytes::Bytes, Error> {
        let answer = self.inner.http.get(url, max).await?;
        match answer.status {
            200 => Ok(answer.body),
            404 => Err(Error::NotFound(format!("{url} doesn't exist"))),
            status => Err(Error::Status {
                url: url.to_owned(),
                status,
            }),
        }
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.inner
            .cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}
