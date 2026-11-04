#!/usr/bin/env python3
"""Builds crates/model/data/tz.tsv, the IANA time zones and their POSIX TZ strings (`just tz`).

OpenWrt's `system.timezone` takes a POSIX TZ string (`CET-1CEST,M3.5.0,M10.5.0/3`), because
its libc reads no zoneinfo; `system.zonename` names the IANA zone. Each line of the table is
`<IANA name>\t<POSIX TZ>`, sorted by name, for every zone and link in tzdata (`tzdata.zi`); the
string is the footer of the zone's compiled TZif file, which says how the zone keeps time now.
The first line names the tzdata version.

Usage: scripts/tz.py [<zoneinfo dir>]   (default /usr/share/zoneinfo)
"""

import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "crates", "model", "data", "tz.tsv")


def footer(path):
    with open(path, "rb") as f:
        data = f.read()
    if not data.startswith(b"TZif") or data[4:5] < b"2":
        sys.exit(f"{path}: not a TZif v2+ file")
    tz = data.rstrip(b"\n").rsplit(b"\n", 1)[-1].decode("ascii")
    if not tz:
        sys.exit(f"{path}: no POSIX footer")
    return tz


def main():
    zoneinfo = sys.argv[1] if len(sys.argv) > 1 else "/usr/share/zoneinfo"
    version, names = None, set()
    with open(os.path.join(zoneinfo, "tzdata.zi"), encoding="utf-8") as f:
        for line in f:
            if line.startswith("# version "):
                version = line.split()[2].removesuffix("-dirty")
            parts = line.split()
            if parts[:1] == ["Z"]:
                names.add(parts[1])
            elif parts[:1] == ["L"]:
                names.add(parts[2])
    if not version or len(names) < 300:
        sys.exit("tzdata.zi lacks a version or zones")
    rows = [f"{name}\t{footer(os.path.join(zoneinfo, name))}" for name in sorted(names)]
    with open(OUT, "w", encoding="utf-8") as f:
        f.write(f"# tzdata {version}\n" + "\n".join(rows) + "\n")
    print(f"{OUT}: {len(rows)} zones (tzdata {version})")


main()
