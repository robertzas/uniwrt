//! ASU's build API (pure): the request for a zero-touch image, and its answers read as an
//! [`ImageBuild`]. `POST api/v1/build` answers 200 with the images of a build it has (cached
//! or just done), 202 while it is queued or building, 500 when it failed; 400 or 422 refuse
//! the request. `GET api/v1/build/<hash>` answers the same way, and 404 once ASU has dropped
//! the build. Images are downloaded from `store/<bin_dir>/<name>`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uniwrt_model::firmware::{FirmwareImage, ImageBuild, ImageBuildState};

use crate::Error;

/// The longest first-boot script ASU takes.
pub(crate) const MAX_DEFAULTS: usize = 40960;
/// How much of the image builder's error output a failure keeps.
const MAX_ERROR: usize = 2000;

/// `POST api/v1/build`'s body.
#[derive(Debug, Serialize)]
pub(crate) struct Request<'a> {
    pub(crate) version: &'a str,
    pub(crate) target: &'a str,
    pub(crate) profile: &'a str,
    /// Added to the profile's defaults (`diff_packages` false), or removed with a leading `-`.
    pub(crate) packages: &'a [String],
    /// The first-boot script (`/etc/uci-defaults`).
    pub(crate) defaults: &'a str,
    pub(crate) client: &'a str,
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    request_hash: Option<String>,
    #[serde(default)]
    detail: Option<Value>,
    #[serde(default)]
    queue_position: Option<u32>,
    #[serde(default)]
    imagebuilder_status: Option<String>,
    #[serde(default)]
    stderr: Option<String>,
    #[serde(default)]
    bin_dir: Option<String>,
    #[serde(default)]
    images: Vec<Image>,
}

#[derive(Deserialize)]
struct Image {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    filesystem: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    sha256: String,
}

/// The kinds people flash first: from a vendor's firmware, then from OpenWrt.
const FIRST: [&str; 2] = ["factory", "sysupgrade"];

/// ASU's answer (`status`, `body`) about the build `hash` (`None` for a new request), on the
/// ASU at `asu` (no trailing `/`).
pub(crate) fn answer(
    asu: &str,
    hash: Option<&str>,
    status: u16,
    body: &[u8],
) -> Result<ImageBuild, Error> {
    let answer: Answer = serde_json::from_slice(body)
        .map_err(|err| Error::Invalid(format!("ASU answered {status} with {err}")))?;
    let detail = answer.detail.as_ref().map(detail_text);
    let refused = || {
        detail
            .clone()
            .unwrap_or_else(|| format!("ASU answered {status}"))
    };
    match status {
        400 | 422 => return Err(Error::Refused(refused())),
        404 => {
            return Err(Error::NotFound(
                "ASU no longer has this build: build it again".to_owned(),
            ));
        }
        429 | 503 | 529 => return Err(Error::Busy(refused())),
        200 | 202 | 500 => {}
        _ => {
            return Err(Error::Status {
                url: format!("{asu}/api/v1/build"),
                status,
            });
        }
    }
    let hash = answer
        .request_hash
        .as_deref()
        .or(hash)
        .filter(|hash| valid_hash(hash))
        .ok_or_else(|| Error::Invalid("ASU's answer names no build".to_owned()))?
        .to_owned();
    let mut build = ImageBuild {
        hash,
        state: ImageBuildState::Failed,
        queue_position: None,
        step: None,
        error: None,
        images: Vec::new(),
    };
    match status {
        200 => {
            let bin_dir = answer
                .bin_dir
                .as_deref()
                .filter(|dir| valid_hash(dir))
                .ok_or_else(|| Error::Invalid("ASU's build has no store directory".to_owned()))?;
            let mut images: Vec<FirmwareImage> = answer
                .images
                .into_iter()
                .filter(|image| valid_name(&image.name) && valid_sha256(&image.sha256))
                .map(|image| FirmwareImage {
                    url: format!("{asu}/store/{bin_dir}/{}", image.name),
                    name: image.name,
                    kind: image.kind,
                    filesystem: image.filesystem,
                    size_bytes: image.size,
                    sha256: image.sha256.to_ascii_lowercase(),
                })
                .collect();
            images.sort_by_key(|image| {
                FIRST
                    .iter()
                    .position(|kind| *kind == image.kind)
                    .unwrap_or(FIRST.len())
            });
            if images.is_empty() {
                build.error = Some("ASU built no image".to_owned());
            } else {
                build.state = ImageBuildState::Done;
                build.images = images;
            }
        }
        202 if detail.as_deref() == Some("queued") => {
            build.state = ImageBuildState::Queued;
            build.queue_position = answer.queue_position;
        }
        202 => {
            build.state = ImageBuildState::Building;
            build.step = answer.imagebuilder_status;
        }
        _ => build.error = Some(failure(detail.as_deref(), answer.stderr.as_deref())),
    }
    Ok(build)
}

/// What a detail says: `FastAPI`'s validation errors are a list of `{msg}`.
fn detail_text(detail: &Value) -> String {
    match detail {
        Value::String(text) => text.clone(),
        Value::Array(errors) => errors
            .iter()
            .filter_map(|error| error.get("msg").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("; "),
        other => other.to_string(),
    }
}

/// A failure's words: ASU's detail, then the end of the image builder's error output.
fn failure(detail: Option<&str>, stderr: Option<&str>) -> String {
    let mut text = detail
        .filter(|d| !d.is_empty())
        .unwrap_or("The build failed")
        .to_owned();
    if let Some(stderr) = stderr.map(str::trim).filter(|s| !s.is_empty()) {
        let start = stderr.len().saturating_sub(MAX_ERROR);
        let start = (start..stderr.len())
            .find(|i| stderr.is_char_boundary(*i))
            .unwrap_or(0);
        text.push('\n');
        text.push_str(&stderr[start..]);
    }
    text
}

/// A build's hash as ASU makes them (hex): it becomes a URL's path.
pub(crate) fn valid_hash(hash: &str) -> bool {
    (8..=64).contains(&hash.len()) && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

fn valid_sha256(sha: &str) -> bool {
    sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'+'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASU: &str = "https://asu.example";
    const HASH: &str = "0123456789abcdef0123456789abcdef";
    const SHA: &str = "a25a05977027deae000b10b025b75ed3e9c12a4b7d42ae57e8d5a8a019493658";

    #[test]
    fn a_queued_build_and_a_running_one() {
        let queued = format!(
            r#"{{"detail":"queued","queue_position":3,"status":202,"request_hash":"{HASH}"}}"#
        );
        let build = answer(ASU, None, 202, queued.as_bytes()).unwrap();
        assert_eq!(
            (build.state, build.queue_position),
            (ImageBuildState::Queued, Some(3))
        );
        assert_eq!(build.hash, HASH);
        let started = r#"{"detail":"started","imagebuilder_status":"building_image","status":202}"#;
        let build = answer(ASU, Some(HASH), 202, started.as_bytes()).unwrap();
        assert_eq!(build.state, ImageBuildState::Building);
        assert_eq!(build.step.as_deref(), Some("building_image"));
    }

    #[test]
    fn a_done_build_lists_its_images_factory_and_sysupgrade_first() {
        let body = format!(
            r#"{{"detail":"done","status":200,"request_hash":"{HASH}","bin_dir":"{HASH}","images":[
            {{"name":"x-initramfs-kernel.bin","type":"kernel","filesystem":"initramfs","size":9,"sha256":"{SHA}"}},
            {{"name":"x-squashfs-sysupgrade.bin","type":"sysupgrade","filesystem":"squashfs","size":10,"sha256":"{SHA}"}},
            {{"name":"x-squashfs-factory.bin","type":"factory","filesystem":"squashfs","size":39,"sha256":"{SHA}"}},
            {{"name":"../escape.bin","type":"factory","sha256":"{SHA}"}},
            {{"name":"x-preloader.bin","type":"preloader.bin","sha256":"short"}}]}}"#
        );
        let build = answer(ASU, None, 200, body.as_bytes()).unwrap();
        assert_eq!(build.state, ImageBuildState::Done);
        let kinds: Vec<&str> = build.images.iter().map(|i| i.kind.as_str()).collect();
        assert_eq!(kinds, ["factory", "sysupgrade", "kernel"]);
        assert_eq!(
            build.images[0].url,
            format!("{ASU}/store/{HASH}/x-squashfs-factory.bin")
        );
        assert_eq!(build.images[0].size_bytes, Some(39));
    }

    #[test]
    fn a_failed_build_says_why() {
        let body = format!(
            r#"{{"detail":"Error: Impossible package selection","status":500,"stderr":"line 1\nmissing kmod-x","request_hash":"{HASH}"}}"#
        );
        let build = answer(ASU, None, 500, body.as_bytes()).unwrap();
        assert_eq!(build.state, ImageBuildState::Failed);
        assert_eq!(
            build.error.as_deref(),
            Some("Error: Impossible package selection\nline 1\nmissing kmod-x")
        );
    }

    #[test]
    fn refusals_and_lost_builds_are_errors() {
        let refused = br#"{"detail":"Unsupported profile: x","status":400}"#;
        assert!(
            matches!(answer(ASU, None, 400, refused), Err(Error::Refused(why)) if why == "Unsupported profile: x")
        );
        let invalid =
            br#"{"detail":[{"loc":["body","target"],"msg":"String should match pattern"}]}"#;
        assert!(
            matches!(answer(ASU, None, 422, invalid), Err(Error::Refused(why)) if why == "String should match pattern")
        );
        let lost = br#"{"status":404,"detail":"could not find provided request hash"}"#;
        assert!(matches!(
            answer(ASU, Some(HASH), 404, lost),
            Err(Error::NotFound(_))
        ));
        let busy = br#"{"status":529,"detail":"server overload"}"#;
        assert!(matches!(answer(ASU, None, 529, busy), Err(Error::Busy(_))));
        assert!(matches!(
            answer(ASU, None, 202, b"{}"),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn hashes_that_fit_a_path() {
        assert!(valid_hash(HASH));
        assert!(!valid_hash("../../etc"));
        assert!(!valid_hash("abc"));
    }
}
