#!/usr/bin/env python3
"""Builds crates/clients/data/oui.bin, the embedded vendor table (`just oui`; D-053).

Reads IEEE's three registries (MA-L, MA-M and MA-S as CSV; downloaded unless a directory with
oui.csv, mam.csv and oui36.csv is given), normalizes the organization names (corporate suffixes
dropped, shouting tamed, well-known brands named as people know them), and writes them in the
compact format that crates/clients/src/oui.rs reads, zlib-compressed:

    b"UOUI" 1                     magic and version
    u32 names, then each name:    u8 length, UTF-8 bytes (sorted; an entry's index points here)
    u32 n, then n entries:        3-byte prefix (MA-L, 24 bits), 3-byte name index
    u32 n, then n entries:        4-byte prefix (MA-M, 28 bits right-aligned), 3-byte name index
    u32 n, then n entries:        5-byte prefix (MA-S, 36 bits right-aligned), 3-byte name index

Integers are little-endian except the prefixes, which are big-endian so that entries sort as
bytes. Entries are sorted by prefix. Private assignments and the IEEE's own blocks are left out.

Usage: scripts/oui.py [<dir with the CSVs>]
"""

import csv
import io
import os
import re
import struct
import sys
import urllib.request
import zlib

SOURCES = {
    "oui.csv": "https://standards-oui.ieee.org/oui/oui.csv",
    "mam.csv": "https://standards-oui.ieee.org/oui28/mam.csv",
    "oui36.csv": "https://standards-oui.ieee.org/oui36/oui36.csv",
}
OUT = os.path.join(os.path.dirname(__file__), "..", "crates", "clients", "data", "oui.bin")
MAX_NAME = 64

# Well-known brands, as people know them: a pattern on the normalized name, then the brand.
BRANDS = [
    (r"^apple\b", "Apple"),
    (r"^samsung\b", "Samsung"),
    (r"^google\b|^nest labs", "Google"),
    (r"^amazon\b", "Amazon"),
    (r"^tp-?link\b", "TP-Link"),
    (r"^xiaomi\b|^beijing xiaomi", "Xiaomi"),
    (r"^huawei\b|^huawei device", "Huawei"),
    (r"^intel\b", "Intel"),
    (r"^espressif\b", "Espressif"),
    (r"^raspberry pi\b", "Raspberry Pi"),
    (r"^sonos\b", "Sonos"),
    (r"^roku\b", "Roku"),
    (r"^lg electronics|^lg innotek", "LG"),
    (r"^sony interactive", "Sony Interactive"),
    (r"^sony\b", "Sony"),
    (r"^microsoft\b", "Microsoft"),
    (r"^nintendo\b", "Nintendo"),
    (r"^ubiquiti\b", "Ubiquiti"),
    (r"^netgear\b", "Netgear"),
    (r"^asustek\b|^asus\b", "ASUS"),
    (r"^dell\b", "Dell"),
    (r"^hewlett packard enterprise|^hpe\b", "HPE"),
    (r"^hewlett[- ]packard|^hp inc", "HP"),
    (r"^lenovo\b", "Lenovo"),
    (r"^realtek\b", "Realtek"),
    (r"^mediatek\b", "MediaTek"),
    (r"^qualcomm\b", "Qualcomm"),
    (r"^murata\b", "Murata"),
    (r"^azurewave\b", "AzureWave"),
    (r"^hon hai\b|^foxconn\b", "Foxconn"),
    (r"^lite-?on\b", "Lite-On"),
    (r"^tuya\b|^hangzhou tuya", "Tuya"),
    (r"^signify\b|^philips lighting", "Signify (Philips Hue)"),
    (r"^ecobee\b", "ecobee"),
    (r"^ring\b", "Ring"),
    (r"^arlo\b", "Arlo"),
    (r"^wyze\b", "Wyze"),
    (r"^allterco\b|^shelly\b", "Shelly"),
    (r"^belkin\b", "Belkin"),
    (r"^synology\b", "Synology"),
    (r"^qnap\b", "QNAP"),
    (r"^brother\b", "Brother"),
    (r"^canon\b", "Canon"),
    (r"^seiko epson\b|^epson\b", "Epson"),
    (r"^garmin\b", "Garmin"),
    (r"^fitbit\b", "Fitbit"),
    (r"^oneplus\b", "OnePlus"),
    (r"^guangdong oppo\b|^oppo\b", "OPPO"),
    (r"^vivo mobile\b|^vivo\b", "vivo"),
    (r"^motorola mobility\b", "Motorola"),
    (r"^nokia\b", "Nokia"),
    (r"^withings\b", "Withings"),
    (r"^irobot\b", "iRobot"),
    (r"^bose\b", "Bose"),
    (r"^peloton\b", "Peloton"),
    (r"^cisco\b", "Cisco"),
    (r"^juniper\b", "Juniper"),
    (r"^aruba\b", "Aruba"),
    (r"^avm\b", "AVM"),
    (r"^zte\b", "ZTE"),
    (r"^gl technologies|^gl\.inet|^gl inet", "GL.iNet"),
    (r"^shenzhen bilian", "LB-Link"),
    (r"^hikvision\b|^hangzhou hikvision", "Hikvision"),
    (r"^zhejiang dahua\b|^dahua\b", "Dahua"),
    (r"^reolink\b", "Reolink"),
    (r"^texas instruments\b", "Texas Instruments"),
    (r"^silicon labs|^silicon laboratories", "Silicon Labs"),
    (r"^nvidia\b", "NVIDIA"),
    (r"^vizio\b", "VIZIO"),
    (r"^tcl\b", "TCL"),
    (r"^hisense\b", "Hisense"),
    (r"^panasonic\b", "Panasonic"),
    (r"^sharp\b", "Sharp"),
    (r"^toshiba\b", "Toshiba"),
    (r"^honeywell\b", "Honeywell"),
    (r"^chamberlain\b", "Chamberlain"),
    (r"^ikea\b", "IKEA"),
    (r"^bsh\b", "Bosch Siemens Home"),
    (r"^robert bosch\b", "Bosch"),
    (r"^miele\b", "Miele"),
    (r"^dyson\b", "Dyson"),
    (r"^valve\b", "Valve"),
    (r"^meta platforms|^facebook\b|^oculus\b", "Meta"),
]

SUFFIX = re.compile(
    r"[\s,.]+("
    r"co\.?,?\s*ltd|company\s+limited|co\.?\s*limited|corporation|corp|incorporated|inc"
    r"|l\.?l\.?c|ltd|limited|gmbh(\s*&\s*co\.?\s*kg)?|ag|s\.?a\.?s|s\.?a|s\.?r\.?l|s\.?p\.?a"
    r"|b\.?v|n\.?v|oy|ab|a/?s|pty\.?\s*ltd|pte\.?\s*ltd|plc|kg|co|k\.?k|sdn\.?\s*bhd"
    r"|private\s+limited|pvt\.?\s*ltd|technology\s+co|technologies\s+co|electronics\s+co"
    r")\.?$",
    re.IGNORECASE,
)
SMALL = {"and", "of", "for", "the", "in", "de", "und"}


def normalize(name):
    name = " ".join(name.replace(" ", " ").split()).strip(" ,.")
    while True:
        shorter = SUFFIX.sub("", name).strip(" ,.")
        if shorter == name or not shorter:
            break
        name = shorter
    if not re.search(r"[a-z]", name) and len(name) > 4:
        words = []
        for i, word in enumerate(name.split(" ")):
            lower = word.lower()
            if i and lower in SMALL:
                words.append(lower)
            elif len(word) <= 3 or any(ch.isdigit() for ch in word):
                words.append(word)
            else:
                words.append("-".join(part.capitalize() for part in lower.split("-")))
        name = " ".join(words)
    for pattern, brand in BRANDS:
        if re.search(pattern, name, re.IGNORECASE):
            return brand
    encoded = name.encode()[:MAX_NAME]
    return encoded.decode("utf-8", "ignore").strip()


def read(directory, file):
    if directory:
        with open(os.path.join(directory, file), encoding="utf-8") as handle:
            return handle.read()
    request = urllib.request.Request(SOURCES[file], headers={"User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(request, timeout=120) as response:
        return response.read().decode("utf-8")


def main():
    directory = sys.argv[1] if len(sys.argv) > 1 else None
    tables = {6: {}, 7: {}, 9: {}}
    for file in SOURCES:
        for row in csv.DictReader(io.StringIO(read(directory, file))):
            assignment = row["Assignment"].strip().upper()
            organization = row["Organization Name"].strip()
            if len(assignment) not in tables or not re.fullmatch(r"[0-9A-F]+", assignment):
                continue
            if organization.lower() in ("private", "") or "ieee registration" in organization.lower():
                continue
            name = normalize(organization)
            if name:
                tables[len(assignment)][int(assignment, 16)] = name
    names = sorted({name for table in tables.values() for name in table.values()})
    index = {name: i for i, name in enumerate(names)}
    assert len(names) < 1 << 24
    out = bytearray(b"UOUI\x01")
    out += struct.pack("<I", len(names))
    for name in names:
        encoded = name.encode()
        out += bytes([len(encoded)]) + encoded
    for digits, width in ((6, 3), (7, 4), (9, 5)):
        table = tables[digits]
        out += struct.pack("<I", len(table))
        for prefix in sorted(table):
            out += prefix.to_bytes(width, "big") + index[table[prefix]].to_bytes(3, "little")
    with open(OUT, "wb") as handle:
        handle.write(zlib.compress(bytes(out), 9))
    counts = ", ".join(f"{len(tables[d])} {label}" for d, label in ((6, "MA-L"), (7, "MA-M"), (9, "MA-S")))
    print(f"{OUT}: {counts}, {len(names)} names, {len(out)} bytes, {os.path.getsize(OUT)} compressed")


if __name__ == "__main__":
    main()
