//! Which of a release's assets are images for a node (pure; D-122): a GitHub release names its
//! images freely, so an asset fits when its name carries the node's profile or board as a
//! whole word followed by its file system or `sysupgrade` (OpenWrt's own naming,
//! `…-glinet_gl-mt6000-squashfs-sysupgrade.bin`), and is a sysupgrade image; an x86 image
//! (which fits any board of its subtarget) when it carries the subtarget (`…-x86-64-…`) and is
//! a `combined` disk image. The image installed is still checked on the node (M7.1c).

/// The node an image must fit.
#[derive(Debug, Clone, Copy)]
pub struct Device<'a> {
    /// The target and subtarget, such as `mediatek/filogic`.
    pub target: &'a str,
    /// The board, such as `glinet,gl-mt6000`.
    pub board: &'a str,
    /// Its image builder profile, such as `glinet_gl-mt6000`, when the catalog knows it.
    pub profile: Option<&'a str>,
}

/// Names that are never images: checksums, signatures, manifests, notes.
const NOT_IMAGES: [&str; 11] = [
    ".sha256",
    ".sha256sum",
    ".sha256sums",
    ".sig",
    ".asc",
    ".manifest",
    ".txt",
    ".json",
    ".buildinfo",
    ".md5",
    ".md",
];

/// What may follow the device in an image's name.
const AFTER_DEVICE: [&str; 6] = ["-squashfs", "-ext4", "-jffs2", "-ubifs", "-sysupgrade", "."];

const FILESYSTEMS: [&str; 4] = ["squashfs", "ext4", "jffs2", "ubifs"];

/// The image's kind (`sysupgrade`, `combined`, `combined-efi`) and file system, when the asset
/// named `name` is an image for `device`.
pub(crate) fn fits(name: &str, device: &Device<'_>) -> Option<(String, Option<String>)> {
    let name = name.to_ascii_lowercase();
    if NOT_IMAGES.iter().any(|suffix| name.ends_with(suffix)) {
        return None;
    }
    let kind = if device.target.starts_with("x86/") {
        let target = device.target.replace('/', "-");
        if !word(&name, &target, word_end) {
            return None;
        }
        if word(&name, "combined-efi", word_end) {
            "combined-efi"
        } else if word(&name, "combined", word_end) {
            "combined"
        } else {
            return None;
        }
    } else {
        let board = device.board.to_ascii_lowercase();
        let mut spellings = vec![board.replace(',', "_"), board.replace(',', "-"), board];
        spellings.extend(device.profile.map(str::to_ascii_lowercase));
        let named = spellings.iter().any(|known| {
            !known.is_empty()
                && word(&name, known, |rest| {
                    AFTER_DEVICE.iter().any(|next| rest.starts_with(next))
                })
        });
        if !named || !word(&name, "sysupgrade", word_end) {
            return None;
        }
        "sysupgrade"
    };
    let filesystem = FILESYSTEMS
        .iter()
        .find(|fs| word(&name, fs, word_end))
        .map(|fs| (*fs).to_owned());
    Some((kind.to_owned(), filesystem))
}

/// Whether `needle` is in `hay` after a separator (or at its start), with `after` holding for
/// what follows it.
fn word(hay: &str, needle: &str, after: impl Fn(&str) -> bool) -> bool {
    hay.match_indices(needle).any(|(at, _)| {
        let starts = at == 0 || matches!(hay.as_bytes()[at - 1], b'-' | b'_' | b'.' | b'/');
        starts && after(&hay[at + needle.len()..])
    })
}

fn word_end(rest: &str) -> bool {
    rest.is_empty() || rest.starts_with(['-', '_', '.'])
}

#[cfg(test)]
mod tests {
    use super::*;

    const MT6000: Device<'static> = Device {
        target: "mediatek/filogic",
        board: "glinet,gl-mt6000",
        profile: Some("glinet_gl-mt6000"),
    };

    fn kind(name: &str, device: &Device<'_>) -> Option<String> {
        fits(name, device).map(|(kind, _)| kind)
    }

    #[test]
    fn openwrt_names_fit_their_board() {
        let found = fits(
            "openwrt-24.10.4-mediatek-filogic-glinet_gl-mt6000-squashfs-sysupgrade.bin",
            &MT6000,
        );
        assert_eq!(
            found,
            Some(("sysupgrade".to_owned(), Some("squashfs".to_owned())))
        );
        // Without the catalog's profile, the board's own spelling still finds it.
        let unprofiled = Device {
            profile: None,
            ..MT6000
        };
        assert!(kind("my-glinet_gl-mt6000-sysupgrade.bin", &unprofiled).is_some());
        assert!(kind("build.glinet,gl-mt6000.sysupgrade.bin", &unprofiled).is_some());
    }

    #[test]
    fn other_boards_variants_and_other_files_dont_fit() {
        for name in [
            "openwrt-24.10.4-mediatek-filogic-glinet_gl-mt3000-squashfs-sysupgrade.bin",
            "openwrt-24.10.4-mediatek-filogic-glinet_gl-mt6000-squashfs-factory.bin",
            "openwrt-24.10.4-mediatek-filogic-glinet_gl-mt6000-initramfs-kernel.bin",
            "openwrt-glinet_gl-mt6000-ubootmod-squashfs-sysupgrade.bin",
            "openwrt-xglinet_gl-mt6000-squashfs-sysupgrade.bin",
            "openwrt-glinet_gl-mt6000-squashfs-sysupgrade.bin.sha256",
            "sha256sums",
        ] {
            assert_eq!(kind(name, &MT6000), None, "{name}");
        }
    }

    #[test]
    fn x86_images_fit_by_their_subtarget() {
        let x86 = Device {
            target: "x86/64",
            board: "qemu-standard-pc-i440fx-piix-1996",
            profile: Some("generic"),
        };
        assert_eq!(
            fits(
                "openwrt-24.10.4-x86-64-generic-squashfs-combined-efi.img.gz",
                &x86
            ),
            Some(("combined-efi".to_owned(), Some("squashfs".to_owned())))
        );
        assert_eq!(
            kind("openwrt-24.10.4-x86-64-generic-ext4-combined.img.gz", &x86).as_deref(),
            Some("combined")
        );
        for name in [
            "openwrt-24.10.4-x86-64-generic-squashfs-rootfs.img.gz",
            "openwrt-24.10.4-x86-generic-generic-squashfs-combined.img.gz",
            "openwrt-24.10.4-x86-64-generic-kernel.bin",
        ] {
            assert_eq!(kind(name, &x86), None, "{name}");
        }
    }
}
