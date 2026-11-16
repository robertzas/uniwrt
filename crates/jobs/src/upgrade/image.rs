//! Which of an upgrade's image files fits the node (D-124): x86 boots a `combined` image with
//! BIOS and a `combined-efi` one with EFI, and every node keeps its root's file system
//! (squashfs or ext4; sysupgrade refuses neither, but the overlay or the data would be lost).

use uniwrt_model::firmware::FirmwareImage;

/// How the node boots and what its root is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Boot {
    /// `/sys/firmware/efi` exists.
    pub(crate) efi: bool,
    /// The root's file system: `squashfs` (with an overlay) or `ext4`; `None` when unknown.
    pub(crate) filesystem: Option<String>,
}

/// The image of `images` that fits `boot`: of the kind it boots (x86's `combined` or
/// `combined-efi`, else `sysupgrade`), on its root's file system, else squashfs, else the first.
pub(crate) fn choose<'a>(images: &'a [FirmwareImage], boot: &Boot) -> Option<&'a FirmwareImage> {
    let combined = images
        .iter()
        .any(|image| image.kind.starts_with("combined"));
    let kind = match (combined, boot.efi) {
        (false, _) => "sysupgrade",
        (true, true) => "combined-efi",
        (true, false) => "combined",
    };
    let fitting: Vec<&FirmwareImage> = images.iter().filter(|image| image.kind == kind).collect();
    let on = |filesystem: &str| {
        fitting
            .iter()
            .copied()
            .find(|image| image.filesystem.as_deref() == Some(filesystem))
    };
    boot.filesystem
        .as_deref()
        .and_then(on)
        .or_else(|| on("squashfs"))
        .or_else(|| fitting.first().copied())
}

/// The root's file system from `/proc/mounts`: `squashfs` when one is mounted at `/rom` (the
/// overlay's lower layer), else what `/` is (`ext4`), unless that is an overlay or tmpfs.
pub(crate) fn root_filesystem(mounts: &str) -> Option<String> {
    let entries = mounts.lines().filter_map(|line| {
        let mut fields = line.split_whitespace();
        let _device = fields.next()?;
        Some((fields.next()?, fields.next()?))
    });
    let mut root = None;
    for (at, filesystem) in entries {
        match (at, filesystem) {
            ("/rom", "squashfs") => return Some("squashfs".to_owned()),
            ("/", "overlay" | "tmpfs" | "rootfs") => {}
            ("/", other) => root = Some(other.to_owned()),
            _ => {}
        }
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(kind: &str, filesystem: Option<&str>) -> FirmwareImage {
        FirmwareImage {
            name: format!("{kind}-{}", filesystem.unwrap_or("none")),
            kind: kind.to_owned(),
            filesystem: filesystem.map(str::to_owned),
            size_bytes: None,
            sha256: String::new(),
            url: String::new(),
        }
    }

    fn boot(efi: bool, filesystem: Option<&str>) -> Boot {
        Boot {
            efi,
            filesystem: filesystem.map(str::to_owned),
        }
    }

    fn chosen(images: &[FirmwareImage], boot: &Boot) -> Option<String> {
        choose(images, boot).map(|image| image.name.clone())
    }

    #[test]
    fn x86_gets_its_boots_kind_on_its_roots_file_system() {
        let images = [
            image("combined", Some("squashfs")),
            image("combined-efi", Some("squashfs")),
            image("combined", Some("ext4")),
            image("combined-efi", Some("ext4")),
        ];
        let pick = |efi, fs| chosen(&images, &boot(efi, fs));
        assert_eq!(pick(true, Some("ext4")).unwrap(), "combined-efi-ext4");
        assert_eq!(pick(false, Some("squashfs")).unwrap(), "combined-squashfs");
        assert_eq!(pick(false, Some("ext4")).unwrap(), "combined-ext4");
        assert_eq!(pick(true, None).unwrap(), "combined-efi-squashfs");
        // Only BIOS images: an EFI node gets none.
        assert_eq!(chosen(&images[..1], &boot(true, None)), None);
    }

    #[test]
    fn other_targets_get_the_sysupgrade_image() {
        let images = [
            image("factory", Some("squashfs")),
            image("sysupgrade", Some("squashfs")),
        ];
        assert_eq!(
            chosen(&images, &boot(true, Some("ext4"))).unwrap(),
            "sysupgrade-squashfs"
        );
        let unnamed = [image("sysupgrade", None)];
        assert_eq!(
            chosen(&unnamed, &Boot::default()).unwrap(),
            "sysupgrade-none"
        );
        assert_eq!(chosen(&images[..1], &Boot::default()), None);
    }

    #[test]
    fn the_root_file_system_is_read_from_the_mounts() {
        let squashfs = "/dev/root /rom squashfs ro,relatime 0 0\nproc /proc proc rw 0 0\n\
                        overlayfs:/overlay / overlay rw,noatime 0 0\n";
        assert_eq!(root_filesystem(squashfs).as_deref(), Some("squashfs"));
        let ext4 = "/dev/root / ext4 rw,noatime 0 0\ntmpfs /tmp tmpfs rw 0 0\n";
        assert_eq!(root_filesystem(ext4).as_deref(), Some("ext4"));
        assert_eq!(root_filesystem("overlay / overlay rw 0 0\n"), None);
        assert_eq!(root_filesystem(""), None);
    }
}
