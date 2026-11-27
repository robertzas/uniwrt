# Insights: logs, events, alerts, metrics, RF, floor plans

The **Insights** group in the sidebar holds the pages you use to see what the network is doing and why: the devices' logs, the events derived from them, the alerts that watch for trouble, the stored metrics, what the radios hear, and floor plans with a coverage heatmap. Most of it works for every adopted device, including devices still in observe mode. Collecting logs, metrics and scans changes no configuration on a device.

Every user can read these pages. Viewers can't change alert rules, maintenance windows, scan settings, the channel plan or floor plans. Alert notifications by ntfy, webhook or email are set up in [Settings](settings.md#notifications).

## Logs

**Logs** shows the system log of every device in one list, newest on top, and adds new entries as they arrive. Entries that uniwrt recognizes carry a short explanation of what they mean. The filters and the time range are part of the page's address, so you can bookmark or share a view. A device's page links here with that device already selected.

The uniwrt agent on each device reads its log, so nothing needs to be set up on the device. Other hosts, such as a switch or a NAS, can send syslog to the controller on port 5514 (UDP or TCP). Their entries appear under **Other hosts**. To move or turn off this receiver, start the controller with `--syslog-listen ADDR:PORT` or `--syslog-listen off` (or set `UNIWRT_SYSLOG_LISTEN`). Logs are kept for 14 days, up to about 1 GiB.

### To search the logs

1. Click the search box, or press `/` anywhere on the page.
2. Type what you look for. The search runs when you stop typing or press Enter. Esc clears it.
3. Matches are highlighted in the list.

The search syntax:

- `word another`: every word must appear.
- `"a phrase"`: the words must appear together, as written.
- `pre*`: words that start with `pre`.
- `-word`: leaves out entries that contain the word.

### To filter the list

- **Severity**: choose **Any severity**, or a level such as **Warning and worse** to hide the less important entries.
- **Sources**: pick one or more devices (under **Devices**) or other hosts (under **Other hosts**). Type in **Filter** to find one.
- **Programs**: pick one or more programs, such as `hostapd` or `netifd`.
- **Explained only**: shows only the entries uniwrt recognizes and explains.
- **Clear filters** removes all of them.
- The range buttons (**15m**, **1h**, **6h**, **24h**, **7d**, **14d**) set how far back the list goes. The default is **1h**.

The **Volume by severity** chart above the list shows how many errors, warnings, info and debug entries were logged over the range. Hover over a bar to see its counts. Click a bar, or drag across several, to zoom the list into that window. **Reset zoom** goes back to the last hour.

### Live tail

With a range button selected, the status line above the list says **Live** and new entries appear at the top as they are logged.

- Click **Pause** to stop new entries from moving the list. **Resume** continues.
- If you scroll down, new entries are held and the status says **Holding new entries**. Click **Show N new entries** to jump back to the top.
- A zoomed window (from the chart) is a **Fixed window**: it doesn't follow new entries.
- Older entries load as you scroll down, or with **Load older entries**. The page keeps at most the newest 10,000 entries. Narrow the filters or the range to see further back.

### To read one entry

1. Click the entry. It expands in place.
2. Read the full message, and for an explained entry, **What it means** with a hint and, where there is one, **Learn more**.
3. The details show when it was **Logged** and **Received**, the **Source**, **Program**, **Severity**, **Facility**, and how it **Arrived by**.
4. Use the actions:
   - **Show context** opens the entries logged around it by every source, oldest first. **Show more** widens the window.
   - **Only this source** or **Only this program** narrows the filters to it.
   - **Copy line** copies the entry as text.

### Saved searches

A saved search keeps a set of filters under a name. It keeps the filters, not the time range. Saved searches are shared: everyone who signs in sees them, viewers included.

To save the current filters:

1. Set the filters you want.
2. Click **Saved**, then **Save this search…**.
3. Enter a **Name** and click **Save**.

To use one, click **Saved** and pick it from the list. The button then shows its name, and a check mark marks it in the list. To delete one, click the trash icon next to it.

## Events

**Events** lists what happened across the network, newest first, grouped by day (**Today**, **Yesterday**, then the date). Events cover devices going offline and coming back, reboots, internet outages, mesh links going down or weak, clients joining, leaving, roaming and being steered, channel changes and congestion, nightly channel plans, configuration changed on a device, and problems recognized in the logs. A problem that repeats within a few minutes is shown once, with a count such as **×3**. A condition that hasn't cleared yet, such as a device that is still offline, is marked **Ongoing**.

Events are kept for 60 days. The filters are part of the page's address, and the device and client pages link here.

### To filter events

- Click the category buttons (**System**, **Internet**, **Mesh**, **Wi-Fi**, **Clients**, **Network**, **Security**) to show only those categories. Click one again to remove it.
- The level menu shows **Any level**, only **Critical**, or a level and above, such as **Warning and above**.
- The device menu shows **Every device** or one device.
- A client's page opens Events filtered to that client. The filter shows as **Client** and its name. Click its **×** to remove it.
- The range buttons go from **1h** to **60d**. The default is **24h**.
- **Clear filters** removes all filters.

With a range button selected, the list is **Live** and new events appear at the top. **Load older events** reads further back.

### To look into an event

1. Click the event to expand it.
2. Read **When**, **Last time** (for repeats), **Times**, **Level**, **Category**, **Device**, **Client** and **Condition**.
3. Use the actions:
   - **Logs around this** opens Logs for the device, five minutes before and after.
   - **Open device** or **Open client** opens its page.
   - **Only this device** or **Only this category** narrows the list.

## Alerts

**Alerts** shows what needs attention. Rules watch the metrics and the events. When a rule matches, it opens an alert for each device, radio, link or client it matches, and the alert stays open until the problem clears. The page has four tabs: **Active**, **History**, **Rules** and **Maintenance**.

The bell in the top bar counts the alerts that need attention: firing, and neither acknowledged nor silenced. Its color follows the worst level. Click it to see the alerts firing. Click one to open it on this page, or click **Open alerts** for the whole list.

### Active

**Active** lists the alerts firing now, the ones that need attention first, then by level. Each row shows the rule's name, what it is about, the cause (for example, a CPU value and the threshold it crossed), when it started and how long it has fired. An alert that came back within its rule's cool-down shows how many times it fired, such as **×2**.

An alert shows **Acknowledged** once an admin acknowledged it, and **Silenced** when it fired during maintenance (recorded, but nobody was notified).

#### To acknowledge an alert

1. Click **Acknowledge** on the alert's row.
2. The alert dims in the list and in the bell until it resolves. Notifiers don't remind you of it.

Viewers can't acknowledge alerts.

#### To look into an alert

1. Click the alert to expand it.
2. Read **Started**, **Last matched**, **Times fired**, **Level**, **About**, **Cause** and, for a condition, **Condition**.
3. Use the actions: **Open device** (or **Open client**), **Events around this**, **Edit the rule**, and **Snooze** to silence alerts for that device or every device for a while.

#### To snooze alerts

1. Click **Snooze alerts** (on the **Active** or **Maintenance** tab), or **Snooze** on an expanded alert.
2. Pick how long: one hour, four hours or one day. From an alert's **Snooze**, pick the length in the row for that device alone or in the row for **Every device**.

Alerts that fire during a snooze are recorded silenced: they appear on the page, but no notifier sends them. Alerts are also silenced while a job runs on their device, and for 5 minutes after.

### History

**History** lists past and current alerts, newest first. Filter by **Level** and **Device**, and choose when they **Started**: **24h**, **7d** (the default), **30d** or **90d**. A resolved alert says why: **Cleared**, **No data** (the metric stopped reporting) or **Rule changed**. Resolved alerts are kept for 90 days.

### Rules

**Rules** lists **Your rules**, then the **Built in** ones. The built-in rules ship with uniwrt and cover devices offline, degraded or rebooting, flash running out, high CPU, memory and temperature, failed jobs, configuration changed on a device, internet down, mesh links down or weak, congested channels, radar, Wi-Fi firmware trouble, problems in the logs and repeated wrong passwords. A few, such as new devices, are off until you turn them on. Each rule reads as a short line that says what it watches, when it fires and on which devices. A built-in rule you changed is marked **Modified**.

Viewers see the rules but can't change them.

#### To turn a rule on or off

1. Click the checkbox at the start of the rule's row.
2. A rule that is off watches nothing, and its open alerts resolve.

#### To add or edit a rule

1. Click **New rule**, or **Edit** on a rule's row.
2. Enter a **Name**, pick a **Level**, and leave **On** checked.
3. Under **Fires on**, choose **A metric** or **Events** and fill in the fields below (see the next sections).
4. Pick a **Cool-down**. If the alert fires again this soon after it resolved, the same alert reopens instead of a new one.
5. Click **Save rule**.

Saving a rule resolves its open alerts. They start again under the new rule if it still matches.

#### A metric rule

- **Watch**: the kind of thing to watch: **The network**, **Devices**, **Interfaces**, **Radios**, **Mesh links**, **Clients** or **WAN probes**.
- **Which**: any device or one device. For interfaces and radios, a **Name** such as `wan` or `radio1` (empty means any). For clients, a **Client MAC** (empty means any). For WAN probes, a **Probe target**.
- **Metric**: the metrics that kind of thing has, such as CPU, signal or download.
- **Fires when**: **Above** or **Below** the **Threshold**. The hint below the field shows how the value reads, for example "Reads as 50 Mbit/s".
- **Clears at**: where it counts as fixed again. Leave it empty to clear on the safe side of the threshold.
- **For at least**: how long the value must stay past the threshold, or **Immediately**.

#### An event rule

- **Events**: the kinds of event to match, in groups. None picked matches any event.
- **Log patterns**: for problems in logs, only these patterns. None picked matches any.
- **Categories**, **Level at least** and **Devices** narrow it further.
- **Times** and **Within**: how many matching events, within how long, fire the alert. With a count of 1, an event that starts a condition (a device offline, for example) keeps its alert open until the condition clears.

#### To reset or delete a rule

- To put a modified built-in rule back to how it shipped, click its reset icon (**Back to how it shipped**).
- To delete one of your own rules, click its trash icon, then **Delete**. Its alerts resolve. Built-in rules can't be deleted; turn them off instead.

### Maintenance

**Maintenance windows** lists the windows **On now** and **Upcoming**. Alerts that fire within a window are recorded silenced: nobody is notified. Use a window when you plan work that will set off alerts, such as rewiring or a firmware upgrade.

#### To schedule a window

1. Under **Schedule a window**, set **Starts** and **Ends**.
2. Under **Devices**, choose **Every device**, or **Some devices** and check the ones it covers.
3. Optionally add a **Note**, such as "Firmware upgrade".
4. Click **Schedule**.

A window lasts at most 30 days and must start within a year. To stop a window early, click **End**. To drop an upcoming one, click **Cancel**. Viewers can't schedule or end maintenance.

## Metrics

**Metrics** charts the stored history of any device, radio, interface, mesh link, client, WAN probe or the whole network. The list on the left, **Series**, groups everything that keeps history. Series appear a minute after a device first reports. History is kept as 15-second averages for two days, 1-minute averages for two weeks and hourly averages for about a year.

### To chart a metric

1. In **Series**, pick what to look at. Type in **Filter** to find it by name, MAC or kind.
2. Click the metric buttons to turn metrics on or off, up to 8 at a time. Metrics with the same unit share one chart.
3. Pick a range, from the last hour to the last year. The chart says which averages it shows.
4. Hover over a chart to read the values at that time.

To zoom in, drag across a chart. **Reset zoom** goes back to the range. **Open its page** opens the device's or client's own page. The address of the page holds your choice, so you can share it.

## RF

**RF** shows what your radios hear, one band at a time: channel occupancy, interference from neighbors' networks, and how your devices hear each other. If your radios use more than one band, choose **2.4 GHz**, **5 GHz** or **6 GHz** at the top. The page opens on 5 GHz.

What the page shows comes from scans. A scan takes a radio off its channel for a few seconds to listen. With **Background scans** on, each radio scans every few hours, only while it carries little traffic. The top of the page says how many radios have been scanned.

### To scan

- To scan every radio now, click **Scan now**. The radios scan one after another.
- To scan one radio, click **Scan** on its card under **Your radios**.
- To change background scans, set **Background scans** to **On** or **Off**, and pick an interval (**Every 4 h** by default).

Viewers can't start scans or change the schedule.

### Spectrum

**Spectrum** draws every network heard on the band as a hill over the channels it uses, as high as its signal. **Your network** is in the accent color, networks marked **Looks like yours** are in the warning color, and each **Neighbor** is muted. DFS channels and channels where radar was seen are marked under the axis. Hover over a hill to highlight that network here and in the table below.

A network that **Looks like yours** is named like your network but isn't one of your devices. It may be an access point waiting for adoption, or one pretending to be yours.

### Your radios

**Your radios** shows a card per radio: its channel, width and power, how busy the channel is, the noise floor, and an interference verdict: **Clean channel**, **Some overlap** or **Crowded**. Each card lists the neighbors that share or overlap its channel, and when it was last scanned and scans next.

### Channel plan

The **Channel plan** card shows what the planner would choose for each radio on this band: channel, width and power. It weighs your other radios, the networks around, how busy each channel is and radar. The preview changes nothing.

1. Click **Preview plan** (later **Plan again**).
2. Read the band's cost before and after, and how much better the plan is. Below 15 % the nightly optimization leaves it alone.
3. In the **Planned settings** table, each radio shows its setting **Now**, the **Planned** one (or **Unchanged**), its **Cost**, and **Why**, for example "Channel fixed in the settings" or "Narrower than 160 MHz: the air is crowded".

What the planner won't move:

- Radios of devices in observe mode stay as they run. Manage a device's Wi-Fi to let the planner move its radios.
- A channel, width or power you fixed in the Wi-Fi settings is kept.
- Radios that carry the mesh share one channel per band, so the planner picks one channel for all of them.

On the 5 GHz band, **DFS channels** sets which radar-shared channels the planner may choose: **None**, **No weather radar** or **All** (the default). Channels where radar was just seen are avoided either way.

### To optimize channels

1. Preview the plan, as above.
2. Click **Use this plan**. It is saved as the site's next revision. Radios left on Auto take its channel, width and power.
3. Click **Review and apply**, then review and apply it like any other change. See [Review and apply](changes.md#review-changes).

Radios that move restart, and their clients reconnect within seconds. Where it can, the apply moves a radio by announcing the switch to its clients, one device at a time. A new mesh channel drops the mesh's links until every device has it, so all devices switch at the same moment. If a device fails to verify, that change rolls back on its own.

### The nightly optimizer

With **Optimize nightly** checked (the default), the planner runs every night on every band from the time set next to it (03:30 by default, the site's local time) for two hours. A band whose plan is at least 15 % better is saved as a revision and applied, with the usual rollback. The optimizer:

- skips the night when changes are waiting to be applied, so it never applies what nobody reviewed;
- waits while a radio that would move carries heavy traffic, a managed device is offline, or another job runs;
- runs at most once a night, and doesn't retry an apply that failed.

Each night's outcome is recorded on the [Events](#events) page: "The nightly optimization applied a new channel plan", or "The nightly optimization skipped a better channel plan" with the reason. In the same window, radios still catching up after a daytime channel switch are restarted, whether or not **Optimize nightly** is on. Viewers see the setting but can't change it.

### Channels

**Channels** lists each 20 MHz channel with how **Busy** it is, the **Neighbors** on it, the **Strongest** of them, your radios on it (**Yours**), and its **DFS** state with any radar hits. Busy is live on your radios' channels, else from the latest scan. Choose **In use** or **All** to see every channel.

### How your devices hear each other

This grid shows the signal at which each device hears each other one: live over the mesh where there is a link, else from the latest scans. The planner and the floor plans' coverage model use these numbers.

### Channel utilization

A chart of how busy each radio's channel was over time, from any transmitter. Pick a range above the chart.

### Networks around

A table of every network heard on the band, most disruptive first: **Network**, **BSSID**, **Channel**, **Signal**, **Security**, **Standard**, **Heard by** and **Impact**. Use **Show** to list **All**, only **Neighbors**, or only **Yours**.

## Floor plans

**Floor plans** puts your devices on a drawing of each floor and shows the Wi-Fi coverage they reach. Floor plans are not part of the network's configuration: nothing goes to a device, so every edit saves at once, without review. Viewers see the plans and the coverage but can't edit them.

### To add a floor

1. Click **Add floor**.
2. Enter a **Name**, a **Level** (0 is the ground floor, basements are negative), and the **Floor-to-floor height** in m or ft.
3. Under **Plan**, choose an image: PNG, JPEG, WebP, GIF or SVG. A drawing, a scan or a photo works.
4. Click **Add floor**.

You can have up to 32 floors. With more than one, pick the floor shown at the top of the page, top floor first. To rename a floor, replace its plan or delete it, click **Edit floor**. Uploading another plan keeps the scale and the devices in place, stretched to its size. Set the scale again if it is a different drawing.

### To set the scale

1. Click **Set scale** (or **Change scale**).
2. Click one end of something whose length you know, such as a wall or a door, then click its other end.
3. Enter **Its real length**, pick the **Unit** (m or ft), and click **Save scale**.

The plan then shows a scale bar, and distances use that unit. Coverage needs a scale.

### Devices

The **Devices** card lists the devices **On this floor**, **Not placed**, and **On other floors**.

- To place a device, click **Place** next to it, then click the plan where it hangs. Esc cancels. Placing a device that is on another floor moves it here.
- To move a device, drag its marker. Or focus it and use the arrow keys; hold Shift for bigger steps.
- To remove a device from the floor, click **Take off**.

Markers are colored by the device's state. To pan the plan, drag it. Zoom with the mouse wheel or the zoom buttons; **Fit the plan** shows it whole.

### Walls

Walls make the coverage heatmap more accurate. Each wall has a material that weakens the signal by a typical amount: **Glass**, **Drywall**, **Wood**, **Brick**, **Concrete** or **Metal**.

1. Click **Walls**. The toolbar opens over the plan.
2. Pick a material.
3. Click to start a wall, then click each corner. Double-click, Enter or Esc ends it. Points snap to wall ends and to 45° steps; hold Alt to draw without snapping.
4. Click **Done** when you finish.

To change a wall, click it: drag its ends, pick another material, or click **Delete wall** (or press Delete). **Undo** (Ctrl+Z) steps back. Every edit saves in the background.

When you open **Walls** on a floor without walls, uniwrt traces the plan's drawing and suggests walls, drawn dashed. Nothing is saved until you accept it.

- Click a suggestion, then **Accept** (or Enter) or **Delete suggestion** (or Delete). Pick a material to change it first.
- **Accept all** or **Delete all** handles every suggestion at once.
- **Suggest walls** traces the plan again, leaving out walls you already drew.

Tracing finds straight walls at right angles and 45°. It works best on a clean, upright drawing, and poorly on a rotated scan or a photo.

### Coverage

The **Coverage** card shows a heatmap of the signal your devices reach across the plan. Choose **Off** or a band. The model is fitted to how your placed devices hear each other, and accounts for the walls and the floors between them.

- The legend runs from **Dead, below −75 dBm** (red) through **Good, −67 dBm** (amber) to **Strong** (green).
- The card gives the floor's dead zones as an area and a share of the floor.
- Where another node would help, a dashed marker shows the spot and the card says how much of the dead zones it would cover. Only spots that hear the mesh at −70 dBm or better on 5 GHz are suggested.
- Dragging a device updates the heatmap as you move it.

The heatmap shows predicted coverage, not where clients are. Until two placed devices hear each other, the card says **Not calibrated yet** and uses typical indoor values. Devices on the band that aren't placed don't count. If the card can't find the floor's outline in the drawing, the whole plan counts.
