# Settings

The **Settings** page holds what the whole controller shares: the site's name, country and time zone, the schedules, where alerts are sent, the history of the site's configuration, the controller's own backups and the support bundle. It has five tabs: **Site**, **Notifications**, **Revisions**, **Backups** and **Support**. Two menus at the foot of the sidebar, **Appearance** and **Account**, complete it.

Everyone can open Settings. Only administrators change anything; viewers see the same tabs read-only, and each tab says what they can't do.

## Site

![The Settings page on its Site tab](https://robertzas.github.io/uniwrt/guide/images/settings.png)

The **Site** tab has two cards: **Site**, with what the whole site shares, and **Schedules**.

- **Name**: the site's name, 1 to 64 characters.
- **Country**: sets the radios' regulatory domain, that is, the channels and transmit power they may use. Pick the country the devices are in: a wrong country is illegal on the air.
- **Time zone**: the devices' clocks and logs, and the times of every schedule (Wi-Fi schedules, restarts, LEDs, the nightly backup).

### To change the site's name, country or time zone

1. Open **Settings**. The **Site** tab opens first.
2. Change **Name**, **Country** or **Time zone**.
3. Select **Save**.
4. The card confirms the revision it saved. Select **Review and apply** to send the change to the devices. See [Review and apply](changes.md#review-changes).

A save changes nothing on the devices by itself. The country and time zone reach them only when you apply.

### Schedules

The **Schedules** card sets when the managed devices restart and when their LEDs are off, for the whole site. The controller runs these schedules itself, in the site's time zone. They don't change the devices' configuration, so a save needs no apply: the controller follows them from the moment you save.

Each schedule lists the devices that follow it under **Followed by**. A device can leave a schedule, or keep its own restart time, on its own page (see [Devices and adoption](devices.md)). Devices in observe mode never follow them.

### To restart the devices on a schedule

1. On the **Site** tab, in **Schedules**, under **Scheduled restarts**, turn on **Restart the devices on a schedule**.
2. Choose the days and the time.
3. Check **Next restart**, which shows when the next one happens.
4. Select **Save**.

The devices restart one at a time, the farthest from the gateway first and the gateway last. A device that is offline or busy waits up to an hour; after that its restart is missed, and an event says why.

### To turn the LEDs off at night

1. In **Schedules**, under **LEDs off**, turn on **Turn the LEDs off at certain times**. It starts with every night from 23:00 to 07:00.
2. Change the days and times, or select **Add a time** for another span.
3. Select **Save**.

Every LED goes dark within these times and comes back as it was afterwards. A restart lights the LEDs until the controller turns them off again.

## Notifications

Alerts always show in the bell at the top of the page. The **Notifications** tab sends them elsewhere too, through notifiers:

- **ntfy**: push notifications on phones and desktops, through ntfy.sh or your own ntfy server.
- **Webhook**: a POST to a URL, with presets for **Discord**, **Slack**, **Microsoft Teams** and **Home Assistant**, uniwrt's own JSON (**uniwrt JSON**) or a **Custom template**.
- **Email**: over SMTP, always encrypted.

Each notifier in the **Notifiers** list shows where it sends, which alerts it passes and how its last delivery went: **Nothing sent yet**, **Delivered** some time ago, or **Failed** with the reason.

You can have at most 20 notifiers. Viewers see the list but can't change it.

### To add a notifier

1. Open **Settings**, then **Notifications**.
2. Select **Add notifier**. With no notifiers yet, you can also pick **ntfy**, **Webhook** or **Email** directly.
3. Under **Send with**, choose the kind. You can't change the kind after you save.
4. Give it a **Name**.
5. Fill in where it sends (see the sections below).
6. Under **Which alerts**, choose what it passes (see [To choose which alerts a notifier sends](#to-choose-which-alerts-a-notifier-sends)).
7. Select **Send a test**, and check that the message arrived.
8. Select **Save**.

### ntfy

- **Server**: `ntfy.sh`, or your own server's address.
- **Topic**: 1 to 64 letters, digits, `-` or `_`. Anyone who knows a topic on a public server can read it, so keep it hard to guess.
- **Sign in**: **None**, **Access token**, or **User and password**, for topics on your own server that need them.

### Webhook

- **Format**: the service you send to. Each preset shows an example of its URL and where to find it in the service.
- **Webhook URL**: services put their token in it, so uniwrt treats it as a secret. After you save, the list shows only its host.
- **Headers**: optional extra HTTP headers, at most 10. Select **Add header**. Header values are kept as secrets.
- **Body template** (with **Custom template** only): your own JSON body with placeholders. Put text placeholders inside quotes (`"{{title}}"`); number placeholders go bare (`{{node}}`) and are `null` when a message has none. Select a placeholder to insert it. **Preview** shows what **An alert** or **A test** makes of the template.

### Email

- **SMTP server** and **Port**.
- **Encryption**: **STARTTLS** (usually port 587) or **TLS** (usually port 465). There is no unencrypted option.
- **The server needs a username and password**: turn it on to give a **Username** and **Password**.
- **From**: an address the server may send from, optionally with a name.
- **To**: up to 10 addresses, separated by commas or spaces.

### To choose which alerts a notifier sends

In the notifier's dialog, under **Which alerts**:

- **Least level**: **Critical**, **Warning**, **Notice** or **Info**. The notifier passes alerts at this level and above.
- **Devices**: **Every device** passes alerts about any device and about the whole network. **Some devices** passes alerts about the devices you tick only; alerts about no device don't pass.
- **When they resolve**: also tell when an alert it reported is over.
- **Reminders**: repeat an alert while it still fires and nobody has acknowledged or snoozed it: **Off**, **Every hour**, **Every 4 hours**, **Every 12 hours** or **Every day**.

### To test a notifier

1. Open the notifier with **Edit**, or start a new one.
2. Select **Send a test**.

The test sends what is in the form, so you can test before you save. For secrets you haven't retyped, it uses the stored ones. The dialog shows **Test sent: check that it arrived.** or the reason it failed.

### To change, pause or remove a notifier

- To pause it, clear the checkbox at the start of its row. A notifier that is off is kept but sends nothing. Tick it again to turn it back on.
- To change it, select **Edit**, make your changes, then select **Save**. A stored secret shows as **Set**: select **Replace** to type a new one, or **Remove** to delete it when you save.
- To remove it, select the trash icon, then **Remove**. Its secrets are deleted too.

### Links in messages

Messages can link to their alert in uniwrt. The **Links in messages** card holds **This controller's address**, the address people open uniwrt at. Until it is saved, it shows **Not set** and messages carry no links.

To set it:

1. Check the address. It is filled in with the address you are using now.
2. Change it if people reach uniwrt at another address, for example a DNS name.
3. Select **Save**.

Select **No links** to stop adding links.

## Revisions

Every save of the site's configuration is a revision: an edit on any page, a restore, an import, or a device removed or replaced. The **Revisions** tab lists them, newest first, with their number, message, author, time and how many changes each made. **Latest** marks the newest. Select **Show older** to load more.

A revision is the configuration the site wants. Devices change only when you apply it. See [Changes: review, apply and history](changes.md).

### To see what a revision changed

1. Open **Settings**, then **Revisions**.
2. Select a revision to open it. It shows its changes from its parent, the revision before it.
3. To compare it with another revision, choose one under **Compare with**.

Each change is **Added**, **Removed** or **Changed**, with its value before and after. Secrets are never shown: a changed password shows as **Changed (secret)**.

### To go back to an earlier configuration

1. Open the revision you want to go back to.
2. Select **Restore**, then **Restore** again to confirm.
3. Its configuration becomes the next revision. Select **Review and apply** to send it to the devices.

Restoring never deletes history: the restored copy is a new revision that names the one it copies. Only administrators restore, and the latest revision has no **Restore** button.

### To export the site

The export is a JSON file of the latest revision, to keep under version control or to import on another controller. It names devices by MAC address.

1. On the **Revisions** tab, select **Export**.
2. Under **What to export**, choose **Without secrets** or **With secrets**. Without secrets, passwords and keys are left out. With secrets, they are sealed under a passphrase you choose.
3. For **With secrets**, type a **Passphrase** of at least 12 characters, and type it again under **Confirm the passphrase**. Keep it: the file can't be opened without it.
4. Select **Download**.

Viewers can export, but only without secrets.

## Import from the devices

**Import from the devices** reads the configuration your adopted devices hold now into the site, as its next revision. Do this before you manage devices you set up by hand, so the first apply keeps your network as it is. It reads devices in observe mode too, and changes nothing on them.

To open it, select **Import from the devices** on the **Revisions** tab, or on the banner on the **Devices** page that says **The site is still the defaults**. A device's drift card can also import just that drift. The page shows what the site would become, device by device, and saves it with **Accept as revision** and its number. See [Changes: review, apply and history](changes.md) for the steps.

Only administrators import.

## Import a site file

**Import a site file** reads a file you exported earlier, from this controller or another one, into the site as its next revision. Devices in the file are matched to adopted devices by MAC address.

### To import a site file

1. On the **Revisions** tab, select **Import a file**.
2. Under **Site file**, choose the `.json` file.
3. If the file holds secrets, type its **Passphrase**. Leave it empty to import without the file's secrets: the site then keeps its own where they match.
4. Select **Preview**.
5. Check the preview:
   - **Changes to the site**: what the site's configuration becomes.
   - **Devices**: which adopted device each device in the file maps to. A device not adopted here is left out with its settings (**Not adopted here: its settings are left out**). An adopted device the file doesn't name keeps its settings.
   - **Secrets**: where each secret comes from: **From the file**, **Kept from the site**, **Missing** or **Made anew on accept**. Give a value for each missing one, or leave it empty to save without it.
6. Optionally type a **Revision message**.
7. Select **Accept as revision** and its number.
8. Select **Review changes**, then review and apply it. See [Review and apply](changes.md#review-changes).

Nothing changes on the devices until you apply. The VPN's server key and the peers' preshared keys are made anew on import, so VPN peers need their configurations again afterwards.

## Backups

The **Backups** tab covers the controller's own backups. Each one is an encrypted file of the whole controller: devices, settings, users, all history, the secrets vault and its key. It doesn't hold firmware files. Backups of each router's configuration are on its device page instead (see [Devices and adoption](devices.md)).

Only administrators take, download and restore backups, because a backup holds every secret. Viewers see the list only.

### Controller backups

Once a passphrase is set, the controller takes a backup every night (the card shows the time, 02:00 site time by default) and keeps the newest 7. The card shows:

- how the last backup went, with the error if it failed
- the backups, newest first, with their time, size and file name
- where they are kept: always the `backups` folder in the controller's data directory

Until a passphrase is set, the card warns **No backups are taken until a passphrase is set.**

To keep copies off the controller's host, for example on a NAS, start the controller with `--backup-dir` (or the `UNIWRT_BACKUP_DIR` environment variable) pointing at a mounted folder. Each backup is then copied there too, the newest 7 are kept there, and each backup shows **Copied** or **Not copied**.

### To set the backup passphrase

1. Open **Settings**, then **Backups**.
2. In the **Passphrase** card, type a **New passphrase** of at least 12 characters.
3. Type it again under **Confirm passphrase**.
4. Select **Set passphrase** (or **Change passphrase** if one is set).

> [!WARNING]
> Nobody can open a backup without its passphrase, and uniwrt never shows it again. Keep it in a password manager. A new passphrase applies only to backups taken from then on: keep the old one for older files.

### To back up now

1. On the **Backups** tab, select **Back up now**.
2. Wait for **Controller backed up**. The new backup tops the list.

### To download a backup

Select **Download** next to the backup. The file is named `uniwrt-backup-….tar.gz.age`.

The file is a standard age-encrypted archive, so you can open it without uniwrt: `age -d uniwrt-backup-….tar.gz.age | tar xz`.

### Restore

Restoring replaces the controller's whole state with a backup's: devices, settings, users, history and secrets. Use it to move the controller to a new host, or to go back after a disaster. The controller restarts to do it. What it replaces is kept in the data directory's `restore/previous` folder until the next restore.

### To restore a backup

1. On the **Backups** tab, in **Restore**, select **Restore from a file…**.
2. Under **Backup file**, choose the `.age` file, then select **Upload**.
3. Type its **Backup passphrase**, then select **Open**. The controller checks the whole file first: a wrong passphrase, a damaged file or a backup from a newer uniwrt is refused.
4. Check what it holds: when it was **Taken**, the version of uniwrt that took it (**By uniwrt**) and its **Database schema**.
5. Select **Restore and restart**.
6. Wait. The page reloads when the controller is back. Sign in with an account from the backup.

If you close the dialog after uploading, the backup waits: the card says **A backup is waiting to be restored.**, and **Continue** reopens it where you left off. Select **Discard** to drop it.

If the controller doesn't come back, check its log. A container needs a restart policy, such as `--restart unless-stopped`, to start again by itself.

### To restore onto a new controller

A new controller can take on a backup before you create an account:

1. Start the new controller, and open it within 10 minutes.
2. Under the setup form, select **Moving a controller? Restore a backup instead.**
3. Follow the steps above. Afterwards, sign in with one of the backup's accounts.

You can also restore from the command line: `uniwrt restore FILE` stages the backup, taking the passphrase from `UNIWRT_BACKUP_PASSPHRASE` or from standard input. The next start puts it in place.

## Support

The **Support** tab makes a support bundle: one file to attach to a bug report. It holds:

- the versions, the site's latest revision (without secrets) and the devices with their status
- the last 24 hours of events and logs, the internet monitor's data and the controller's own log
- each device's details and, for those online, its configuration and a snapshot of its status, addresses, routes, neighbors and logs

### To download a support bundle

1. Open **Settings**, then **Support**.
2. Select **Download the bundle**. Reading every online device can take a minute.
3. Look the file over before you share it.

Passwords and keys are masked, but logs and configurations still name your devices, networks and clients. Only administrators can download the bundle.

## Appearance

The **Appearance** menu, at the foot of the sidebar, changes how uniwrt looks in this browser. The choice is remembered per browser, not per account.

- **Skin**: **Graphite pro** (neutral, crisp and dense), **Midnight glass** (translucent layers and neon accents) or **UniFi-familiar** (airy cards and an icon rail).
- **Theme**: **System** follows your device's setting; **Light** and **Dark** fix it.

You can also switch both from the command palette.

## Account

The **Account** menu, at the foot of the sidebar under your initial, shows who is signed in and their role. Select **Sign out** to end your session.

There are two roles:

- **Administrator**: manages everything. The first account, created at first start, is an administrator.
- **Viewer**: sees every page but can't change anything, and can't open pages or downloads that reveal secrets.

A session ends after 7 days without use, or after 30 days in all. uniwrt then asks you to sign in again.

There is no page yet to add users or change a password.
