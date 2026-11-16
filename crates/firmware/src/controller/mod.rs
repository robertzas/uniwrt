//! The controller's own releases (D-121, D-123): the newest release of uniwrt's repo with a
//! tarball for this binary's target ([`Releases::controller`]), and staging it
//! ([`GitHub::stage_controller`]): the tarball downloaded and hashed, checked against GitHub's
//! digest and against the release's Sigstore attestation, and its binary written next to the
//! others as `uniwrt-<version>`. Running it is the launcher's (the server crate's) business.

mod tarball;
#[cfg(test)]
mod tests;

use std::{fmt, path::Path, path::PathBuf, str::FromStr};

use data_encoding::HEXLOWER;
use serde::Deserialize;
use serde_json::value::RawValue;
use uniwrt_core::Timestamp;

use crate::{
    Error, GitHub,
    github::{Releases, api_headers, refusal},
    sigstore::{Provenance, Subject, Untrusted},
};

/// uniwrt's repo: its releases are the controller's.
pub const CONTROLLER_REPO: &str = "robertzas/uniwrt";
/// The largest tarball downloaded and binary extracted (today about 15 and 33 MB).
const MAX_TARBALL: u64 = 96 << 20;
const MAX_BINARY: u64 = 192 << 20;
/// The largest answer of GitHub's attestations API (one bundle is about 12 KB).
const MAX_ATTESTATIONS: usize = 1 << 20;

/// A release version, `X.Y.Z` (D-008: every release is one, no pre-release part).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(pub u32, pub u32, pub u32);

impl FromStr for Version {
    type Err = ();

    /// `X.Y.Z`, or a tag `vX.Y.Z`; nothing else (a dev build's `0.0.0-dev.3+abc` isn't one).
    fn from_str(text: &str) -> Result<Self, ()> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.').map(|part| {
            (!part.is_empty() && part.len() <= 9 && part.bytes().all(|b| b.is_ascii_digit()))
                .then(|| part.parse::<u32>().ok())
                .flatten()
        });
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(Some(x)), Some(Some(y)), Some(Some(z)), None) => Ok(Self(x, y, z)),
            _ => Err(()),
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// A release of the controller with a tarball for one target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerRelease {
    pub version: Version,
    pub tag: String,
    pub name: String,
    /// The release's page (its notes).
    pub notes: String,
    pub published_at: Option<Timestamp>,
    pub tarball: String,
    pub url: String,
    pub size: u64,
    /// GitHub's SHA-256 of the tarball (hex).
    pub sha256: String,
}

impl ControllerRelease {
    /// The binary's path inside the tarball.
    fn member(&self, target: &str) -> String {
        format!("uniwrt-{}-{target}/uniwrt", self.version)
    }
}

/// A binary staged: its version, its path, and what its attestation told.
#[derive(Clone, Debug)]
pub struct Staged {
    pub version: Version,
    pub path: PathBuf,
    pub provenance: Provenance,
}

/// The staged binary's file name in its directory.
pub fn binary_name(version: Version) -> String {
    format!("uniwrt-{version}")
}

impl Releases {
    /// The newest release (by version) that isn't a prerelease, is tagged `vX.Y.Z` and has a
    /// tarball for `target` with GitHub's SHA-256.
    pub fn controller(&self, target: &str) -> Option<ControllerRelease> {
        self.list
            .iter()
            .filter(|release| !release.prerelease)
            .filter_map(|release| {
                let version: Version = release.tag.parse().ok()?;
                let tarball = format!("uniwrt-{version}-{target}.tar.gz");
                let asset = release.assets.iter().find(|asset| asset.name == tarball)?;
                Some(ControllerRelease {
                    version,
                    tag: release.tag.clone(),
                    name: release.name.clone(),
                    notes: release.notes.clone(),
                    published_at: release.published_at,
                    sha256: asset.sha256.clone()?,
                    tarball,
                    url: asset.url.clone(),
                    size: asset.size,
                })
            })
            .max_by_key(|release| release.version)
    }
}

#[derive(Deserialize)]
struct Attestations<'a> {
    #[serde(borrow)]
    attestations: Vec<Attestation<'a>>,
}

#[derive(Deserialize)]
struct Attestation<'a> {
    #[serde(borrow)]
    bundle: &'a RawValue,
}

impl GitHub {
    /// The Sigstore bundles GitHub keeps for `sha256` (hex) in `repo` (each a JSON document).
    pub async fn attestations(
        &self,
        repo: &str,
        sha256: &str,
        token: Option<&str>,
    ) -> Result<Vec<Vec<u8>>, Error> {
        if !crate::valid_sha256(sha256) || !crate::valid_repo(repo) {
            return Err(Error::Invalid(format!("{repo}@{sha256} isn't an artifact")));
        }
        let url = format!(
            "{}/repos/{repo}/attestations/sha256:{sha256}",
            self.inner.base
        );
        let answer = self
            .inner
            .http
            .get_with(&url, &api_headers(token)?, MAX_ATTESTATIONS)
            .await?;
        match answer.status {
            200 => {}
            404 => return Ok(Vec::new()),
            status => return Err(refusal(repo, token.is_some(), status, &answer)),
        }
        let parsed: Attestations<'_> = serde_json::from_slice(&answer.body).map_err(|err| {
            Error::Invalid(format!("GitHub's attestations are unreadable: {err}"))
        })?;
        Ok(parsed
            .attestations
            .iter()
            .map(|attestation| attestation.bundle.get().as_bytes().to_vec())
            .collect())
    }

    /// Downloads `release`'s tarball for `target` into `dir`, checks it against GitHub's
    /// SHA-256 and against an attestation `verify` accepts (in production
    /// [`crate::sigstore::verify`] with the embedded root and uniwrt's CI), and writes its
    /// binary as `dir/uniwrt-<version>`. Nothing but that binary stays in `dir`.
    pub async fn stage_controller(
        &self,
        release: &ControllerRelease,
        target: &str,
        dir: &Path,
        verify: impl Fn(&[u8], Subject<'_>) -> Result<Provenance, Untrusted>,
    ) -> Result<Staged, Error> {
        let storage = |err: std::io::Error| Error::Storage(format!("{}: {err}", dir.display()));
        tokio::fs::create_dir_all(dir).await.map_err(storage)?;
        let download = dir.join(format!(".{}.part", release.tarball));
        let result = self
            .stage_from(release, target, dir, &download, verify)
            .await;
        let _ = tokio::fs::remove_file(&download).await;
        result
    }

    async fn stage_from(
        &self,
        release: &ControllerRelease,
        target: &str,
        dir: &Path,
        download: &Path,
        verify: impl Fn(&[u8], Subject<'_>) -> Result<Provenance, Untrusted>,
    ) -> Result<Staged, Error> {
        let got = self
            .inner
            .http
            .download(&release.url, MAX_TARBALL, download)
            .await?;
        let hex = HEXLOWER.encode(&got.sha256);
        if hex != release.sha256 || got.size != release.size {
            return Err(Error::Untrusted(format!(
                "{} downloaded as {} bytes with SHA-256 {hex}, GitHub lists {} bytes with {}",
                release.tarball, got.size, release.size, release.sha256
            )));
        }
        let bundles = self.attestations(CONTROLLER_REPO, &hex, None).await?;
        let subject = Subject {
            name: &release.tarball,
            sha256: &got.sha256,
        };
        let mut refusals = Vec::new();
        let provenance = bundles
            .iter()
            .find_map(|bundle| {
                verify(bundle, subject)
                    .map_err(|err| refusals.push(err.0))
                    .ok()
            })
            .ok_or_else(|| {
                Error::Untrusted(if refusals.is_empty() {
                    format!("GitHub has no attestation of {}", release.tarball)
                } else {
                    refusals.join("; ")
                })
            })?;

        let name = binary_name(release.version);
        let part = dir.join(format!(".{name}.part"));
        let path = dir.join(&name);
        let (archive, member, out) = (download.to_owned(), release.member(target), part.clone());
        let extracted = tokio::task::spawn_blocking(move || {
            tarball::extract(&archive, &member, &out, MAX_BINARY)
        })
        .await
        .map_err(|err| Error::Storage(err.to_string()))?;
        let found = match extracted {
            Ok(found) => found,
            Err(err) => {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(Error::Invalid(format!("{}: {err}", release.tarball)));
            }
        };
        if !found {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(Error::Invalid(format!(
                "{} holds no {}",
                release.tarball,
                release.member(target)
            )));
        }
        tokio::fs::rename(&part, &path)
            .await
            .map_err(|err| Error::Storage(format!("{}: {err}", path.display())))?;
        Ok(Staged {
            version: release.version,
            path,
            provenance,
        })
    }
}
