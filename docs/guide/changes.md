# Changes: review, apply and history

uniwrt keeps your whole network's configuration as one document on the controller. Every page that changes something saves a new revision of that document, and nothing reaches your devices until an admin reviews the result and applies it. This chapter covers that cycle: saving, reviewing, applying, rolling back, and the history of revisions. It also covers drift (changes made on a device behind the controller's back), taking over an existing configuration, moving the site to another controller, and the UCI explorer for settings no page covers.

## How changes work

- **Every save is a revision.** When you save on any page, a toast says **Saved as revision N. Review and apply it to change the devices.** The devices keep running what they have.
- **Only managed devices change.** A newly adopted device is in observe mode: uniwrt reads it but never writes to it. An apply changes a device only after an admin turns management on for it and chooses which packages uniwrt writes, on the device's **Config** tab (see [Devices and adoption](devices.md)).
- **Pending changes show in the top bar.** While the latest revision has not reached every managed device, admins see a **Review changes** button with the number of devices to update. While an apply runs, it reads **Applying…**. The command palette also has **Review and apply changes**.
- **Applies are safe to try.** Each device applies the change as a transaction with a timer armed. A device that isn't confirmed in time puts its old configuration back by itself (see [Automatic rollback](#automatic-rollback)).
- **The history is kept.** Every revision stays in **Settings** › **Revisions**, where you can compare any two and restore an older one (see [Revisions](#revisions)).

Viewers can't save, review or apply changes; they see the history and can export the site.

## Review changes

The **Review changes** page shows what applying the latest revision does to each device. uniwrt logs in to every managed device just now and compares what it runs with what it would write. Nothing changes while you look.

The top card names the revision and counts the devices: **N devices change**, **N up to date**, **N in observe mode**, and **N can't apply**. Below it:

- **Impact** sums up what you will notice: Wi-Fi restarting and clients reconnecting, a network restart that pauses traffic for a few seconds, DHCP and DNS or the firewall reloading, services that start or stop, a device that moves to a new address, and changes made by hand that this apply overwrites. Its last line says how long a device has to be confirmed before it rolls back.
- **In the order they apply** lists the devices that change, farthest from the gateway first.
- **Left alone** lists the devices that are up to date or in observe mode.

### To review pending changes

1. Click **Review changes** in the top bar.
2. Wait while uniwrt reads every managed device.
3. Read the **Impact** card, then each device's card.
4. Click **Review again** to read the devices again, for example after you fixed something.

If someone saves another revision while you review, the page says **Revision N was saved since this review: review again to apply it.** Click **Review again** before you apply.

### What a device's card shows

- Its action: **Changes**, **Up to date**, **Observe mode** or **Can't apply**, and how many hops it is from the gateway.
- **Rolls back after … unless confirmed**: the device's rollback timer for this apply.
- **Installs first**: packages the new configuration needs that the device lacks. They come from OpenWrt's package feeds (or the controller's mirror, see [Offline packages](updates.md#offline-packages)) before anything else changes. A rollback keeps them installed, with their services off.
- **Started at boot**: services the apply turns **on** or **off**.
- A note when the device moves to another address, and when the controller's own network changes address. In that case the gateway keeps the old address beside the new one until the controller's host has an address on the new network, and its LAN ports go down for a few seconds so wired devices ask for a new address.
- **The *package* package**: one expandable diff per UCI package, with each section **Added**, **Removed**, **Changed** or **Moved**, option by option. Secrets show as **secret**; the device receives the real value.
- **Drift**: a package that was changed on the device by hand. This apply rewrites it (see [Drift](#drift)).
- **Raw overrides**: UCI sections the site keeps for this device and writes as they are (see [UCI explorer](#uci-explorer)).

### When a device can't apply

A device marked **Can't apply** shows **Why it can't apply**, with each problem and the setting it comes from. Nothing applies until every managed device can, so **Apply revision N** stays disabled.

1. Fix each listed problem on the page it names, or turn management off for that device.
2. Click **Review again**.

> [!NOTE]
> Every managed device must be reachable for an apply to start. A managed device that can't be reached is marked **Can't apply**, with the reason: bring it back online, or turn its management off.

## Apply

Applying writes the reviewed revision to every managed device that changes. All devices stage the change and check it first, then all apply it with their timers armed, then the controller checks every device, then it confirms them.

### To apply a revision

1. On **Review changes**, check that no device is marked **Can't apply**.
2. Click **Apply revision N**.
3. Follow the progress: the steps (such as **Plan the changes**, **Stage on every device**, **Apply**, **Check every device** and **Confirm**) and each device's state (**Waiting**, **Staged**, **Applied, checking**, **Passed its checks**, **Confirmed**, **Rolled back** or **Failed**).
4. When it ends, the card says **Revision N is applied and confirmed.** Click **Review again** to return to the review.

You can leave the page while an apply runs. It continues, the top bar shows **Applying…**, and a toast says how it went.

If no device changes, the review says so, and applying only records that every managed device holds the revision.

### Automatic rollback

Each device applies the change as a transaction. Before it changes anything, it saves a copy of the packages it replaces and arms a timer. Then:

- The controller logs in to each device again over a new connection and checks it against how it was before the apply: the gateway's internet connection and DNS, the radios that were up, the mesh links, and that the controller can still reach it.
- Only when every device passes does the controller confirm them.
- A device that isn't confirmed before its timer runs out restores its old configuration by itself and reloads. This also happens when a change cuts the device off from the controller, and after an unexpected restart.

So a change that breaks the connection to a device undoes itself, usually within two minutes, without anyone on site. When an apply rolls back, its result reads **The changes were rolled back**, and every device goes back to what it had.

If some devices confirmed and others rolled back, the result reads **The changes applied only in part**. Follow **Open the history** to keep the result or restore the previous revision and apply it.

### To discard changes

When the latest revision is newer than what the devices hold, you can go back to the revision they hold.

1. On **Review changes**, click **Discard changes**.
2. Confirm with **Discard changes**.

The revision the devices hold becomes the next revision, so nothing is pending. The discarded revisions stay in the history.

## Revisions

**Settings** › **Revisions** lists every revision of the site, newest first. Each row shows its number, its message, its kind (**Defaults**, **Edit**, **Restore**, **Import**, **From a file**, **Device removed** or **Device replaced**), who saved it, when, and how many changes it holds. Click **Show older** for earlier ones.

The tab also has **Export**, and for admins **Import a file** and **Import from the devices**.

### To see what a revision changed

1. Click the revision to open it.
2. Read the changes from its parent: each setting **Added**, **Removed** or **Changed**, with **Before** and **After**. Secrets show only as changed.
3. To compare with another revision, pick it under **Compare with**.

### To roll back to an older revision

1. Open the revision you want back.
2. Click **Restore**, then **Restore** again to confirm.
3. Click **Review and apply** in the toast, or **Review changes** in the top bar, and apply it.

Restoring copies the old revision's configuration into a new revision; nothing changes on the devices until you apply it. Admins can restore any revision but the latest.

## Drift

Drift is a change made on a managed device after uniwrt confirmed it, by anyone but uniwrt: LuCI, `uci` on the command line, an editor, or the device page's **Terminal** tab. uniwrt watches the packages it writes on each managed device, from the first apply that confirms them. It doesn't watch devices in observe mode or packages it doesn't manage.

Admins see a **Drift** button in the top bar with the number of drifted packages that wait for a decision. It opens a list of them; each opens its device's **Config** tab, and **Review & overwrite** opens the review.

The **Drift** card on a device's **Config** tab shows each drifted package with its diff from what uniwrt confirmed, or **File removed** when the file is gone.

### To handle a drift

On the device's **Config** tab, in the **Drift** card, choose one:

- **Import**: take the device's version into the site. It opens an import of that package alone (see [To import a drift](#to-import-a-drift)).
- **Overwrite**: open **Review changes**, where the package is marked **Drift**. Applying rewrites it as the site has it.
- **Acknowledge**: accept it as it is until the file changes again. The next apply still rewrites it.
- **Mute**: stop watching this package on this device. Muted packages are listed under **Not watched**; click **Watch again** to resume.

A drift ends by itself when the file goes back to what uniwrt confirmed. Each drift raises an event and the built-in drift alert.

## Import from the devices

**Import from the devices** takes over an existing network. It reads the configuration your adopted devices hold, in observe mode too, and turns it into the site's next revision, so that the first apply keeps your network as it is. Nothing changes on the devices.

> [!TIP]
> Import before you turn management on. When the site is still the defaults and devices are adopted, the **Devices** page shows a banner, **The site is still the defaults**, with **Import from the devices**.

### To import from the devices

1. Go to **Settings** › **Revisions** and click **Import from the devices**.
2. Wait while uniwrt reads every adopted device.
3. Read the preview:
   - **Changes to the site**: what the site's configuration becomes. Secrets go to the vault and are never shown.
   - Each device's state (**Imported**, **Failed** or **Not read**) and **N% in the model**, the share of its options the site's model holds. A device not read keeps the settings the site has for it.
   - Each device's sections: **Mapped** (the site's model holds it), **Kept raw** (written back as it is) or **Dropped** (an access point keeps no firewall zones, WAN or DHCP server of its own).
   - **What the first apply would still change**: the few options uniwrt writes its own way.
4. Optionally type a **Revision message**.
5. Click **Accept as revision N**.
6. Click **Review changes**, check that little or nothing changes, then turn management on and apply.

If the preview has errors, fix them on the devices and click **Read again**. If another revision was saved meanwhile, read the devices again before accepting.

### To import a drift

1. On the device's **Config** tab, in the **Drift** card, click **Import**.
2. The page reads only that package as the device holds it now; every other package is taken as the latest revision has it, so the changes shown are the drift's alone.
3. Click **Accept as revision N**, then review and apply it. Applying confirms the package as the device holds it, which ends the drift.

Click **Import everything instead** to import every device and package.

## Export the site

Export downloads the latest revision as a JSON file, to keep under version control or to import on another controller. Devices are named by MAC address.

### To export the site

1. Go to **Settings** › **Revisions** and click **Export**.
2. Under **What to export**, choose:
   - **Without secrets**: passwords and keys are left out; an import keeps the importing site's own.
   - **With secrets** (admins only): passwords and keys, sealed under a passphrase of at least 12 characters, typed twice. Keep it: the file can't be opened without it.
3. Click **Download**.

## Import a site file

This page reads a file from **Export**, made here or on another controller, into the site as its next revision. Only admins import.

### To import a site file

1. Go to **Settings** › **Revisions** and click **Import a file**.
2. Under **Site file**, choose the `.json` file.
3. If the file holds secrets, type the **Passphrase** chosen at export, or leave it empty to import without them.
4. Click **Preview**.
5. Read the preview:
   - **Changes to the site**, and the warnings and errors.
   - **Devices**: each device in the file takes the adopted device with the same MAC address. A device not adopted here has its settings left out; an adopted device the file doesn't name keeps its settings.
   - **Secrets**: where each value comes from: **From the file**, **Kept from the site**, **Missing**, or **Made anew on accept** (the VPN's keys; the VPN's peers then need their configurations again). Type a value for each missing secret that a setting requires.
6. Optionally type a **Revision message**, then click **Accept as revision N**.
7. Review and apply the new revision.

Nothing changes on the devices until you apply, and device modes stay as they are. A file from an older uniwrt is upgraded; a file from a newer one is refused.

## UCI explorer

The device page's **UCI** tab shows every package in the device's `/etc/config`, read live from the device, with secrets masked. Use it for settings no page covers. Each package is marked:

- **Rendered**: uniwrt writes this package whole; an edit overrides what it renders.
- **Patched**: the device's own file, which the site's overrides patch on each apply.
- **Device's own**: a file uniwrt doesn't touch; an edit makes the site patch it.

Edits here don't go to the device directly. Each one is saved to the site as an override of this device, as a new revision, and reaches the device when you review and apply it.

### To edit a section

1. Open the device and the **UCI** tab. Use **Search the configuration** to find a name or value.
2. On the section, click **Edit**.
3. Change, add (**Add option**) or delete options. Tick **List** for a list option, one item per line. A secret option is sealed in the vault; leave it empty to keep its value.
4. Click **Save to the site**.
5. Review and apply the new revision.

To add a section, click **Add section** on the package, give its **Type** and optionally a **Name**, then save. To delete one, click **Delete section** and confirm with **Delete**.

### To drop an override

A section with overrides shows them under **The site's overrides of this section**. Click **Drop** to take one out of the site. In a rendered package, uniwrt's own values return; in a patched one, the device keeps what it holds now. **Overrides not on the device** lists the overrides that find no section on the device yet: not applied yet, a removal already applied, or a package the device lacks.

Limits:

- In observe mode the tab is read-only; turn management on in the **Config** tab to edit.
- Viewers can't edit the configuration.
- An anonymous section with no plain option to find it by, or one that matches an earlier section of its type, can't be edited here: change it in the **Terminal** tab.
- If the site changed since the tab read the device, click **Read again** before editing.
