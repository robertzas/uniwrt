# uniwrt user guide

uniwrt runs your OpenWrt routers as one network: a gateway and its access points, adopted, configured, watched and upgraded from one web app. This guide is for the person running it. It walks through each page of the app in the order of its sidebar, and says how to do the common tasks on each one.

## Start here

1. [Install and first start](install.md): run the controller, trust its certificate, create the admin account.
2. [Getting started](getting-started.md): the few ideas the rest of the guide relies on, the first-run wizard for a new router, or taking over a network you already have.
3. [Devices and adoption](devices.md): bring the access points in, then let the controller manage them.
4. [Changes: review, apply and history](changes.md): how every change is previewed, applied safely and rolled back.

## Pages

| Page | Chapter |
|---|---|
| Dashboard, Topology | [Dashboard and topology](dashboard-topology.md) |
| Devices | [Devices and adoption](devices.md) |
| Clients | [Clients](clients.md) |
| Wi-Fi | [Wi-Fi and mesh](wifi.md) |
| Networks | [Networks](networks.md) |
| Internet | [Internet](internet.md) |
| Security, VPN | [Security and VPN](security-vpn.md) |
| Insights: Logs, Events, Alerts, Metrics, RF, Floor plans | [Insights](insights.md) |
| Updates | [Updates](updates.md) |
| Settings | [Settings](settings.md) |
| Review, history | [Changes: review, apply and history](changes.md) |

When something doesn't work, see [Troubleshooting](troubleshooting.md).

## Safety in one paragraph

A device starts in **observe** mode: the controller reads it and changes nothing. Once you turn management on, every change you make is saved as a revision, shown as a per-device diff for review, and applied as a transaction. If a device loses contact with the controller after a change, it undoes the change by itself. Any earlier revision can be restored.
