# Updates

The **Updates** page keeps uniwrt and your devices current. From top to bottom it shows the **Controller** card (uniwrt itself), the **Firmware** table (the OpenWrt release on each device and the upgrades on offer), **Rolling upgrades** of several devices, the site's **Image sources**, your **Uploaded images**, and **Offline packages**, the controller's package mirror.

Everyone can read the page. Only admins install, upgrade or change settings here.

## Controller

![The Updates page: the controller's release, the devices' firmware and the upgrades on offer](https://robertzas.github.io/uniwrt/guide/images/updates.png)

The **Controller** card shows the version you run (**Running uniwrt …**) and, when there is one, the newest release for your platform (**uniwrt … is available**) with its date and release notes. Otherwise it says **This controller runs the newest release.**

New releases come from uniwrt's GitHub releases. Before a release runs, the controller checks the download's size and SHA-256 against what GitHub records and verifies the build's signed attestation: the release must have been built by uniwrt's own CI workflow from its main branch, and logged in the public Sigstore transparency log. A download that fails any check is never run.

### To install a new controller release

1. On the **Controller** card, click **Install**.
2. Follow the progress: **Downloading and verifying …**, **Checking …**, then **Restarting into …: the page reconnects by itself.**
3. When the card says **uniwrt … is running**, click **Reload** to load the new interface.

The new release first runs as a trial, with a copy of the database taken before. If it fails its health check, the controller goes back to the release it ran, with the data it had. The card then says the release **failed its health check … and was rolled back**, and that release is never offered again.

### To install releases automatically

1. On the **Controller** card, turn on **Install new releases automatically**.
2. Pick when the nightly window opens under **Window opens at** (02:30 by default, in the site's time zone).

Each night, in that one-hour window and while no job runs, the controller installs the newest release that has been out for at least a day. It skips the night when it can't reach GitHub.

A controller can't install releases when it is a development build, runs in demo mode, or was started with `--self-upgrade off` (`UNIWRT_SELF_UPGRADE=off`). The card says which. Then update the container image instead (see [Install and first start](install.md)).

## Firmware

The **Firmware** table lists every device with its **Running** release, the **Upgrade** on offer, its image **Source**, and its **State**: **Up to date**, **Upgrade available**, **Builds available** (a GitHub repo has releases to choose from) or **Unknown**. When no upgrade is offered, the row says why, for example a snapshot build or a board without an image. **Check again** reads the catalog again.

uniwrt offers two kinds of OpenWrt upgrade, each with a link to its **Release notes**:

- **Update**: a newer release of the same branch, fixes only.
- **New release**: the next branch, with new features. uniwrt never skips a branch, so going from an older branch to the newest can take more than one upgrade.

You can also open a device's upgrade from the **Firmware** card on its **Config** tab.

### To change a device's image source

1. In the device's row, under **Source**, click **Change**.
2. Choose one:
   - **Site default**: OpenWrt's releases, or the site's GitHub repo if one is set under [Image sources](#image-sources).
   - **OpenWrt, built by ASU**: OpenWrt's releases, built with the device's packages.
   - **OpenWrt, official image**: OpenWrt's releases as published; uniwrt installs your packages after the upgrade.
   - **Another GitHub repo**: the releases of a repo with builds for this device. Type it under **GitHub repo** as `owner/name` or its link.
3. Click **Save**.

### To upgrade one device

1. In the device's row, click **Prepare upgrade**. uniwrt reads the device's packages again (nothing changes on it) and shows the plan.
2. Under **Image**, pick an OpenWrt release (an **Update** or a **New release**), a GitHub release, or an uploaded image. The choice shows only when there is more than one.
3. For an OpenWrt release, choose under **Image source**:
   - **Built by ASU**: OpenWrt's Attended Sysupgrade service builds an image with your packages baked in. The plan lists your packages carried over, those **Renamed by OpenWrt**, those **Added by uniwrt (mesh essentials)**, and those not in the new release, which are left out of the image. Install those again afterwards if you still need them.
   - **Official image**: OpenWrt's image as published, for when ASU is busy or down. uniwrt installs your packages after the upgrade, so the device needs internet access then (or the controller's [offline packages](#offline-packages)).

   A GitHub release or an uploaded image is flashed as it is, with the packages it was built with.
4. Check the image's size against the free memory in `/tmp`, where it is staged. The dialog warns when there is little room.
5. For ASU, click **Build the image** and wait until it says **The image is ready to install.**
6. Click **Install**, read what will happen, then click **Install now**.
7. Follow the upgrade's steps in the dialog. You can close it; the upgrade goes on.

The upgrade runs these steps: **Check the device and pick its image**, **Back up the configuration**, **Download the image** (checked against its SHA-256 and size), **Copy it to the device and test it** (`sysupgrade -T`), **Write the image, keeping the settings**, **Restart and come back**, **Install its packages** (official images only), **Check it against before** (its internet connection and DNS on the gateway, its radios, mesh links and clients), **Watch it for a few minutes** (three minutes without new errors in its log), and **Read its packages again**.

The device is offline for a few minutes while it restarts; its clients reconnect to other devices or wait.

- A failure before the image is written changes nothing on the device.
- If the device doesn't come back, the result reads **The device didn't come back**. Its settings are backed up on the controller; recover it on the spot with OpenWrt's failsafe mode or TFTP recovery.
- Devices in observe mode can't be upgraded from here: turn their management on first (see [Devices and adoption](devices.md)).

The ASU service is `sysupgrade.openwrt.org` by default. To use your own, start the controller with `--asu-url` (`UNIWRT_ASU_URL`); `off` turns it off. Nothing secret is sent to it.

## Rolling upgrades

A rollout upgrades several devices one at a time, each the way a single upgrade does: an optional canary first, then the devices farthest from the gateway, and the gateway last. A failure stops the rollout on that device.

### To roll out upgrades

1. On the **Rolling upgrades** card, click **Roll out upgrades**.
2. Under **Devices**, choose the devices. Managed devices with an OpenWrt upgrade are preselected; devices in observe mode can't be chosen.
3. For each device, pick its image (its GitHub releases, each OpenWrt release built by ASU or as the official image, and its uploads). uniwrt reads each plan and sums it up; click **Plan** to see it whole.
4. Check the **Order**. The canary comes first and the gateway last; the controller orders the rest by distance from the gateway, farthest first.
5. Optionally set up a **Canary** and a **Maintenance window** (below).
6. Click **Upgrade N devices**, then **Start now**.

Only one rollout runs at a time.

### Canary

With two or more devices, turn on **Canary** to upgrade one device first and watch it. Pick the **Canary device** (**The farthest device** by default; never the gateway) and how long to **Watch it for** (30 minutes to 24 hours; 1 hour by default). To pass, the canary must stay online without restarting, accept the controller's login, and run its new release. Otherwise the rollout halts before any other device starts.

### Maintenance window

Turn on **Maintenance window** and set **From** and **To** (03:30 to 05:30 by default, in the site's time zone; it may cross midnight). Devices start their upgrade only inside the window. Outside it, the rollout waits for the next opening.

### To follow or control a rollout

The card shows the rollout as it runs: its state (**Running**, **Paused**, **Halted**, **Done** or **Cancelled**), what it waits for (the canary, the window, or another job on the next device), and each device with its image, its state (**Waiting**, **Preparing the image**, **Upgrading**, **Upgraded**, **Failed** or **Skipped**) and the running device's steps.

- **Pause** holds the next device; the one upgrading finishes first.
- **Resume** goes on. After a device failed, it tries that device again from its checks. After the canary failed its check, it goes on with the rest; check the canary first.
- **Skip *device*** leaves the failed device out, with its old firmware, and goes on with the next one.
- **Cancel rollout** ends it. Devices not upgraded yet keep their firmware; upgraded ones keep the new one. You can't cancel while a device upgrades: pause, and cancel once it is done.

Devices already upgraded keep their new firmware whatever happens next. A rollout continues after a controller restart. **History** lists the last 20 rollouts; open one to see how it ended.

## Image sources

The **Image sources** card sets where the site's upgrades come from: OpenWrt's releases, or a GitHub repo with your own builds. A device whose source is a repo is offered that repo's newest five releases with a sysupgrade image named for its board, checked against the SHA-256 that GitHub records. A release asset without a recorded SHA-256 isn't offered.

### To use your own GitHub builds

1. On the **Image sources** card, type the **GitHub repo** as `owner/name` or its link. Leave it empty for OpenWrt's releases.
2. Turn on **Offer prereleases** to also offer releases marked as prereleases.
3. Optionally give an **Access token**: a fine-grained token with read access to the repo's contents. A private repo needs one, and it lifts GitHub's limit of 60 requests an hour. The token is kept in the vault; use **Replace token** or **Remove token** to change it.
4. Click **Save**.

Devices whose source is **Site default** then follow this repo.

## Uploaded images

The **Uploaded images** card holds sysupgrade images you built yourself. uniwrt reads which boards each image fits from its metadata (for an x86 image, from its file name) and offers it to those devices. Each image shows how many devices it fits.

### To upload an image

1. On the **Uploaded images** card, click **Upload image**.
2. Choose the file (`.bin`, `.img`, `.gz` or `.itb`).

An image that fits no board is refused. The controller keeps at most 16 images of up to 256 MiB each; click **Delete** on one to make room. An uploaded image is flashed as it is.

## Offline packages

The controller keeps the packages your devices use, for each release and architecture they run or are offered, and serves them to the devices. Adoption, applies, package changes and upgrades then work without internet. The mirror refreshes every 12 hours while the controller is online. Devices fall back to their own feeds when the controller can't be reached.

The card shows when the mirror last refreshed, how much it keeps, and one row per release, target, architecture and package manager, marked **In use** or **Upgrade on offer**. Open a row to see each feed's index, packages kept and listed, size and state, the packages that couldn't be kept, and names not in any feed. **Official images kept** lists the images kept for the next release, so an official-image upgrade works offline too.

### To change the mirror

- **Serve packages to devices** is on by default. Turn it off to have devices use their own feeds; the copies kept stay on disk.
- **Keep whole feeds** keeps every package of every feed, not only what your devices use: about 3 GB for each release and architecture. Confirm with **Keep whole feeds**. Turning it off again deletes what no device uses.
- **Refresh now** refreshes the mirror at once.

Viewers see the mirror's settings but can't change them.
