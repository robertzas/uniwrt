//! Rekor v1's transparency log entry: the log's key from the trust root, its signed entry
//! timestamp, the logged body matching the envelope and the certificate, and the inclusion proof
//! (RFC 9162) up to a checkpoint the log signed.

use data_encoding::{BASE64, HEXLOWER};
use ring::digest::{Context, SHA256, digest};
use serde::Deserialize;

use super::{TrustedRoot, Untrusted, cert::Key, root::Log, untrusted};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Entry {
    log_index: String,
    log_id: LogId,
    kind_version: KindVersion,
    integrated_time: String,
    inclusion_promise: Option<Promise>,
    inclusion_proof: Option<Proof>,
    canonicalized_body: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogId {
    key_id: String,
}

#[derive(Deserialize)]
struct KindVersion {
    kind: String,
    version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Promise {
    signed_entry_timestamp: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Proof {
    log_index: String,
    root_hash: String,
    tree_size: String,
    hashes: Vec<String>,
    checkpoint: Checkpoint,
}

#[derive(Deserialize)]
struct Checkpoint {
    envelope: String,
}

/// What the entry must have logged.
pub(super) struct Signed<'a> {
    pub(super) payload: &'a [u8],
    pub(super) signature: &'a [u8],
    pub(super) certificate: &'a [u8],
}

/// A verified entry: when the log integrated it (Unix seconds).
pub(super) struct Logged {
    pub(super) integrated_time: i64,
}

pub(super) fn verify(
    entry: &Entry,
    signed: &Signed<'_>,
    root: &TrustedRoot,
) -> Result<Logged, Untrusted> {
    if entry.kind_version.kind != "dsse" || entry.kind_version.version != "0.0.1" {
        return Err(untrusted(format!(
            "Rekor entries of kind {} {} aren't supported",
            entry.kind_version.kind, entry.kind_version.version
        )));
    }
    let number = |text: &str, what: &str| {
        text.parse::<i64>()
            .ok()
            .filter(|n| *n >= 0)
            .ok_or_else(|| untrusted(format!("the entry's {what} isn't a number")))
    };
    let integrated_time = number(&entry.integrated_time, "time")?;
    let log_index = number(&entry.log_index, "index")?;
    let key_id = decode(&entry.log_id.key_id, "log ID")?;
    let log = root
        .logs
        .iter()
        .find(|log| log.key_id == key_id && log.valid.holds_at(integrated_time))
        .ok_or_else(|| untrusted("the entry was logged by no Rekor log of the trust root"))?;
    if !log.ecdsa_p256 {
        return Err(untrusted(
            "only Rekor logs with ECDSA P-256 keys are supported",
        ));
    }
    let key = Key::from_spki(&log.key)?;

    // The signed entry timestamp: the log's promise over the body, the time and the index.
    let promise = entry
        .inclusion_promise
        .as_ref()
        .ok_or_else(|| untrusted("the entry carries no signed entry timestamp"))?;
    let canonical = format!(
        r#"{{"body":"{}","integratedTime":{integrated_time},"logID":"{}","logIndex":{log_index}}}"#,
        entry.canonicalized_body,
        HEXLOWER.encode(&key_id),
    );
    let set = decode(&promise.signed_entry_timestamp, "signed entry timestamp")?;
    if !key.verifies(canonical.as_bytes(), &set) {
        return Err(untrusted("the entry's signed entry timestamp doesn't hold"));
    }

    let body = decode(&entry.canonicalized_body, "body")?;
    check_body(&body, signed)?;
    let proof = entry
        .inclusion_proof
        .as_ref()
        .ok_or_else(|| untrusted("the entry carries no inclusion proof"))?;
    check_inclusion(&body, proof, log, &key)?;
    Ok(Logged { integrated_time })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Body {
    api_version: String,
    kind: String,
    spec: Spec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Spec {
    payload_hash: Hash,
    signatures: Vec<BodySignature>,
}

#[derive(Deserialize)]
struct Hash {
    algorithm: String,
    value: String,
}

#[derive(Deserialize)]
struct BodySignature {
    signature: String,
    verifier: String,
}

/// The logged body names the payload's hash, the envelope's signature and the certificate.
fn check_body(body: &[u8], signed: &Signed<'_>) -> Result<(), Untrusted> {
    let body: Body = serde_json::from_slice(body)
        .map_err(|err| untrusted(format!("the entry's body is unreadable: {err}")))?;
    if body.api_version != "0.0.1" || body.kind != "dsse" {
        return Err(untrusted("the entry's body isn't a DSSE entry"));
    }
    let payload_hash = HEXLOWER.encode(digest(&SHA256, signed.payload).as_ref());
    if body.spec.payload_hash.algorithm != "sha256" || body.spec.payload_hash.value != payload_hash
    {
        return Err(untrusted("the entry logged another payload"));
    }
    let [logged] = body.spec.signatures.as_slice() else {
        return Err(untrusted("the entry must log exactly one signature"));
    };
    if decode(&logged.signature, "logged signature")? != signed.signature {
        return Err(untrusted("the entry logged another signature"));
    }
    let pem = decode(&logged.verifier, "logged certificate")?;
    if pem_der(&pem).as_deref() != Some(signed.certificate) {
        return Err(untrusted("the entry logged another certificate"));
    }
    Ok(())
}

/// The DER of a PEM certificate (one block).
fn pem_der(pem: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(pem).ok()?;
    let inner = text
        .trim()
        .strip_prefix("-----BEGIN CERTIFICATE-----")?
        .strip_suffix("-----END CERTIFICATE-----")?;
    let base64: String = inner.split_ascii_whitespace().collect();
    BASE64.decode(base64.as_bytes()).ok()
}

fn check_inclusion(body: &[u8], proof: &Proof, log: &Log, key: &Key) -> Result<(), Untrusted> {
    let index = proof
        .log_index
        .parse::<u64>()
        .map_err(|_| untrusted("the proof's index isn't a number"))?;
    let size = proof
        .tree_size
        .parse::<u64>()
        .map_err(|_| untrusted("the proof's tree size isn't a number"))?;
    let root = decode(&proof.root_hash, "root hash")?;
    let path = proof
        .hashes
        .iter()
        .map(|hash| decode(hash, "proof hash"))
        .collect::<Result<Vec<_>, _>>()?;
    if !included(&leaf_hash(body), index, size, &path, &root) {
        return Err(untrusted("the entry's inclusion proof doesn't hold"));
    }
    let checkpoint = parse_checkpoint(&proof.checkpoint.envelope)
        .ok_or_else(|| untrusted("the proof's checkpoint is malformed"))?;
    if checkpoint.size != size || checkpoint.root != root {
        return Err(untrusted("the proof's checkpoint is of another tree"));
    }
    let signed = checkpoint.signatures.iter().any(|(hint, signature)| {
        log.key_id.get(..4) == Some(hint.as_slice()) && key.verifies(checkpoint.text, signature)
    });
    if !signed {
        return Err(untrusted("the proof's checkpoint isn't signed by the log"));
    }
    Ok(())
}

/// RFC 6962's leaf hash.
pub(super) fn leaf_hash(data: &[u8]) -> Vec<u8> {
    let mut context = Context::new(&SHA256);
    context.update(&[0]);
    context.update(data);
    context.finish().as_ref().to_vec()
}

/// RFC 6962's interior node hash.
pub(super) fn node_hash(left: &[u8], right: &[u8]) -> Vec<u8> {
    let mut context = Context::new(&SHA256);
    context.update(&[1]);
    context.update(left);
    context.update(right);
    context.finish().as_ref().to_vec()
}

/// RFC 9162 §2.1.3.2: whether `path` proves `leaf` at `index` in the tree of `size` with `root`.
pub(super) fn included(leaf: &[u8], index: u64, size: u64, path: &[Vec<u8>], root: &[u8]) -> bool {
    if index >= size {
        return false;
    }
    let (mut fn_, mut sn) = (index, size - 1);
    let mut hash = leaf.to_vec();
    for sibling in path {
        if sn == 0 {
            return false;
        }
        if fn_ & 1 == 1 || fn_ == sn {
            hash = node_hash(sibling, &hash);
            if fn_ & 1 == 0 {
                while fn_ & 1 == 0 && fn_ != 0 {
                    fn_ >>= 1;
                    sn >>= 1;
                }
            }
        } else {
            hash = node_hash(&hash, sibling);
        }
        fn_ >>= 1;
        sn >>= 1;
    }
    sn == 0 && hash == root
}

/// A signed note (the checkpoint): the text signed, its tree size and root, and each
/// signature's key hint with the signature.
struct Note<'a> {
    text: &'a [u8],
    size: u64,
    root: Vec<u8>,
    signatures: Vec<(Vec<u8>, Vec<u8>)>,
}

fn parse_checkpoint(envelope: &str) -> Option<Note<'_>> {
    let split = envelope.find("\n\n")?;
    let text = &envelope[..=split];
    let mut lines = text.lines();
    let _origin = lines.next().filter(|origin| !origin.is_empty())?;
    let size = lines.next()?.parse().ok()?;
    let root = BASE64.decode(lines.next()?.as_bytes()).ok()?;
    let signatures = envelope[split + 2..]
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("\u{2014} ")?;
            let (_name, signature) = rest.rsplit_once(' ')?;
            let signature = BASE64.decode(signature.as_bytes()).ok()?;
            (signature.len() > 4).then(|| (signature[..4].to_vec(), signature[4..].to_vec()))
        })
        .collect();
    Some(Note {
        text: text.as_bytes(),
        size,
        root,
        signatures,
    })
}

fn decode(text: &str, what: &str) -> Result<Vec<u8>, Untrusted> {
    BASE64
        .decode(text.as_bytes())
        .map_err(|_| untrusted(format!("the entry's {what} isn't base64")))
}
