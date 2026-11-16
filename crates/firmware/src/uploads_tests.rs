use uniwrt_core::Timestamp;

use super::*;
use crate::testing::sysupgrade_image;

async fn upload(
    uploads: &Uploads,
    name: &str,
    image: &[u8],
    at: i64,
) -> Result<FirmwareUpload, Error> {
    let mut receiving = uploads.receive(name).await?;
    // In parts smaller than the tail, as a request body comes.
    for part in image.chunks(1000) {
        receiving.write(part).await?;
    }
    receiving.finish(Timestamp::from_unix_ms(at)).await
}

#[tokio::test]
async fn an_image_is_kept_under_its_hash_with_its_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let uploads = Uploads::open(dir.path().join("firmware")).await.unwrap();
    let image = sysupgrade_image("glinet,gl-mt6000", "24.10.9", 200_000);
    let kept = upload(&uploads, "C:\\dl\\custom.bin", &image, 1000)
        .await
        .unwrap();
    assert_eq!(kept.name, "custom.bin");
    assert_eq!(kept.size_bytes, image.len() as u64);
    assert_eq!(kept.target, "mediatek/filogic");
    assert_eq!(kept.boards, ["glinet,gl-mt6000"]);
    assert_eq!(kept.version.as_deref(), Some("24.10.9"));
    assert_eq!(
        kept.sha256,
        hex(ring::digest::digest(&ring::digest::SHA256, &image).as_ref())
    );
    let path = uploads.path(&kept.sha256).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), image);

    // The same image again is the one kept; another is listed first.
    let again = upload(&uploads, "other-name.bin", &image, 2000)
        .await
        .unwrap();
    assert_eq!(again, kept);
    let newer = sysupgrade_image("glinet,gl-mt6000", "25.12.6", 1000);
    let newer = upload(&uploads, "newer.bin", &newer, 3000).await.unwrap();
    let listed = uploads.list().await.unwrap();
    assert_eq!(listed, [newer.clone(), kept.clone()]);

    // Reopened, the images are there; deleted, they are gone.
    let reopened = Uploads::open(dir.path().join("firmware")).await.unwrap();
    assert_eq!(reopened.list().await.unwrap().len(), 2);
    assert!(reopened.remove(&kept.sha256).await.unwrap());
    assert!(!reopened.remove(&kept.sha256).await.unwrap());
    assert!(!path.exists());
    assert_eq!(reopened.list().await.unwrap(), [newer]);
}

#[tokio::test]
async fn what_isnt_an_image_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let uploads = Uploads::open(dir.path().to_owned()).await.unwrap();
    let refused = upload(&uploads, "notes.txt", b"hello", 0).await;
    assert!(matches!(refused, Err(Error::Invalid(_))), "{refused:?}");
    assert!(matches!(
        upload(&uploads, "empty.bin", b"", 0).await,
        Err(Error::Invalid(_))
    ));
    // Dropped half-way, as when a client goes away.
    let mut receiving = uploads.receive("half.bin").await.unwrap();
    receiving.write(b"abc").await.unwrap();
    drop(receiving);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    assert_eq!(uploads.list().await.unwrap(), []);
}

#[tokio::test]
async fn hashes_that_cant_be_a_file_name_find_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let uploads = Uploads::open(dir.path().to_owned()).await.unwrap();
    assert!(uploads.path("../etc/passwd").is_none());
    assert_eq!(uploads.get("ABC").await.unwrap(), None);
    assert!(!uploads.remove(&"0".repeat(64)).await.unwrap());
    assert!(valid_sha256(&"a".repeat(64)));
    assert!(!valid_sha256(&"A".repeat(64)));
}
