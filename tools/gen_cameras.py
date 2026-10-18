#!/usr/bin/env python3
"""Generate src-tauri/cameras.toml: every camera with a CHDK build.

Sources:
  * CHDK autobuild indexes (stable + trunk): which cameras/firmwares can be downloaded
  * CHDK source (platform/*): USB id, OS, DryOS release, DIGIC, multi-partition support
  * the end of one build zip per camera (HTTP range request): whether PS.FI2 is included
  * src-tauri/cameras.overrides.toml: hand-written details, which win

Usage:
  python3 tools/gen_cameras.py                 # downloads the CHDK source
  python3 tools/gen_cameras.py --src ~/chdk     # use a local CHDK checkout
"""

import argparse
import concurrent.futures
import io
import json
import re
import sys
import tarfile
import tempfile
import tomllib
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "src-tauri" / "cameras.toml"
OVERRIDES = ROOT / "src-tauri" / "cameras.overrides.toml"

INDEXES = {
    "stable": "https://mighty-hoernsche.de/",
    "trunk": "https://mighty-hoernsche.de/trunk/",
}
SOURCE_TARBALL = "https://codeload.github.com/Anda2012/CHDK/tar.gz/refs/heads/main"
UA = {"User-Agent": "chdk-sd-creator camera list generator"}

DIGIC = {20: "DIGIC II", 30: "DIGIC III", 40: "DIGIC 4", 41: "DIGIC 4+",
         50: "DIGIC 5", 60: "DIGIC 6", 70: "DIGIC 7"}


def fetch(url, headers=None):
    req = urllib.request.Request(url, headers={**UA, **(headers or {})})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


# ---------- builds ----------

BUILD_RE = re.compile(r'href="(bins/([a-z0-9_]+)-(\d{3}[a-z])-(\d+\.\d+\.\d+)-(\d+)\.zip)"')


def builds():
    """{camera_id: {"firmwares": set, "stable": bool, "zip": url}}"""
    cams = {}
    for channel, page in INDEXES.items():
        html = fetch(page).decode("utf-8", "replace")
        for href, cam, fw, _ver, _rev in BUILD_RE.findall(html):
            c = cams.setdefault(cam, {"firmwares": set(), "stable": False, "zip": None})
            c["firmwares"].add(fw)
            if channel == "stable":
                c["stable"] = True
            # Prefer a stable zip for the FI2 check.
            if c["zip"] is None or channel == "stable":
                c["zip"] = page + href
    return cams


def has_fi2(zip_url):
    """Read only the zip's central directory (at its end) and look for PS.FI2."""
    tail = fetch(zip_url, {"Range": "bytes=-16384"})
    return b"PS.FI2" in tail


# ---------- source ----------

def source_dir(arg):
    if arg:
        return Path(arg).expanduser()
    tmp = Path(tempfile.mkdtemp(prefix="chdk-src-"))
    print("Downloading CHDK source…", file=sys.stderr)
    data = fetch(SOURCE_TARBALL)
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
        wanted = [m for m in tar.getmembers()
                  if "/platform/" in m.name and m.isfile()
                  and m.name.endswith(("makefile.inc", "platform_camera.h"))]
        tar.extractall(tmp, members=wanted, filter="data")
    return next(tmp.iterdir())


def var(text, name):
    m = re.search(rf"^\s*{name}\s*=\s*([^\s#]+)", text, re.M)
    return m.group(1) if m else None


def platform_info(src, cam, firmwares):
    pdir = src / "platform" / cam
    info = {}
    mk = (pdir / "makefile.inc").read_text(errors="replace") if (pdir / "makefile.inc").exists() else ""
    digic = var(mk, "DIGIC")
    if digic and int(digic) in DIGIC:
        info["processor"] = DIGIC[int(digic)]
    hdr = pdir / "platform_camera.h"
    if hdr.exists():
        info["multipart"] = bool(re.search(r"^\s*#define\s+CAM_MULTIPART\s+1\b", hdr.read_text(errors="replace"), re.M))
    # These are set either per firmware (sub/<fw>/makefile.inc) or per camera.
    subs = [pdir / "sub" / fw / "makefile.inc" for fw in sorted(firmwares)]
    text = "\n".join(s.read_text(errors="replace") for s in subs if s.exists()) + "\n" + mk
    pid = var(text, "PLATFORMID")
    if pid:
        info["usb_pid"] = int(pid, 0)
    info["dryos"] = (var(text, "PLATFORMOS") or "dryos") == "dryos"
    osver = var(text, "PLATFORMOSVER")
    # Some ports reuse another camera's build ("# SX270HS - uses SX280HS as source camera").
    src_cam = re.search(r"uses (\w+) as source camera", mk, re.I)
    if not osver and src_cam and src_cam.group(1).lower() != cam:
        other = src_cam.group(1).lower()
        other_subs = src / "platform" / other / "sub"
        fws = [d.name for d in other_subs.iterdir()] if other_subs.is_dir() else []
        other_info = platform_info(src, other, fws)
        info["dryos"] = other_info.get("dryos", True)
        osver = other_info.get("osver")
    if osver:
        info["osver"] = int(osver)
    return info


# ---------- names ----------

SPECIAL = {
    "n": "PowerShot N", "n_facebook": "PowerShot N Facebook", "tx1": "PowerShot TX1",
    "m3": "EOS M3", "m10": "EOS M10",
    "g1x": "PowerShot G1 X", "g5x": "PowerShot G5 X", "g7x": "PowerShot G7 X",
    "g7x2": "PowerShot G7 X Mark II",
    "sx1": "PowerShot SX1 IS", "sx10": "PowerShot SX10 IS", "sx20": "PowerShot SX20 IS",
    "sx30": "PowerShot SX30 IS",
}


def model(token):
    """'sx130is' -> 'SX130 IS', 'a590' -> 'A590', 'elph100hs' -> 'ELPH 100 HS'."""
    m = re.fullmatch(r"([a-z]+?)(\d+)(is|hs)?", token)
    if not m:
        return token.upper()
    prefix, num, suffix = m.groups()
    sep = " " if prefix in ("ixus", "elph") else ""
    out = f"{prefix.upper()}{sep}{num}"
    return f"{out} {suffix.upper()}" if suffix else out


def names(cam):
    """(display name, aliases, series)"""
    if cam in SPECIAL:
        name = SPECIAL[cam]
        series = "EOS" if name.startswith("EOS") else series_of(cam)
        return name, [], series
    first, *rest = cam.split("_")
    if first.startswith("ixus"):
        aliases = []
        hs = ""
        for alt in rest:
            if alt.startswith("sd"):
                aliases += [f"PowerShot {alt.upper()}", f"Digital ELPH {alt.upper()}"]
            elif alt.startswith("elph"):
                aliases.append(f"PowerShot {model(alt)}")
                if alt.endswith("hs"):
                    hs = " HS"
        base = {"ixusizoom": "IXUS i zoom", "ixusw": "IXUS Wireless"}.get(first) or model(first)
        return base + hs, aliases, "IXUS / ELPH"
    return f"PowerShot {model(first)}", [], series_of(cam)


def series_of(cam):
    for prefix, series in (("sx", "PowerShot SX"), ("a", "PowerShot A"), ("g", "PowerShot G"),
                           ("s", "PowerShot S"), ("d", "PowerShot D")):
        if re.match(rf"{prefix}\d", cam):
            return series
    return "Other"


# ---------- output ----------

def toml_value(v):
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, int):
        return str(v)
    if isinstance(v, str):
        return json.dumps(v, ensure_ascii=False)
    if isinstance(v, list):
        if not v:
            return "[]"
        if sum(len(str(x)) for x in v) > 70:
            return "[\n" + "".join(f"  {toml_value(x)},\n" for x in v) + "]"
        return "[" + ", ".join(toml_value(x) for x in v) + "]"
    raise TypeError(v)


FIELD_ORDER = ["id", "name", "aliases", "series", "year", "os", "processor", "usb_pid",
               "firmwares", "stable", "fi2", "fat32_autoboot", "multipart", "ver_req",
               "max_card_gb", "fw_check", "notes"]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--src", help="local CHDK source checkout (default: download)")
    args = ap.parse_args()

    overrides = tomllib.loads(OVERRIDES.read_text()) if OVERRIDES.exists() else {}
    available = builds()
    print(f"{len(available)} cameras have builds", file=sys.stderr)
    src = source_dir(args.src)

    with concurrent.futures.ThreadPoolExecutor(8) as pool:
        fi2 = dict(zip(available, pool.map(lambda c: has_fi2(available[c]["zip"]), available)))

    cameras = []
    for cam in sorted(available):
        b = available[cam]
        p = platform_info(src, cam, b["firmwares"])
        dryos = p.get("dryos", True)
        osver = p.get("osver")
        name, aliases, series = names(cam)
        entry = {
            "id": cam,
            "name": name,
            "aliases": aliases,
            "series": series,
            "os": f"DryOS r{osver}" if dryos and osver else ("DryOS" if dryos else "VxWorks"),
            "processor": p.get("processor"),
            "usb_pid": p.get("usb_pid"),
            "firmwares": sorted(b["firmwares"]),
            "stable": b["stable"],
            "fi2": fi2[cam],
            # DryOS release 47 (2011) and later can autoboot from FAT32.
            "fat32_autoboot": bool(dryos and osver and osver >= 47),
            "multipart": p.get("multipart", False),
            "ver_req": "ver.req" if dryos else "vers.req",
        }
        entry.update(overrides.get(cam, {}))
        cameras.append(entry)

    lines = [
        "# GENERATED by tools/gen_cameras.py from the CHDK autobuild and source. Do not edit:",
        "# put hand-written details in cameras.overrides.toml and re-run the generator.",
        "#",
        "# id              CHDK platform name, as used in autobuild file names",
        "# firmwares       firmware versions with a CHDK build (stable or trunk)",
        "# stable          false = only development (trunk) builds exist",
        "# fi2             build includes PS.FI2, so Menu > Firmware Update can load CHDK",
        "# fat32_autoboot  camera can autoboot from FAT32 (DryOS r47+); else FAT16 is needed",
        "# multipart       after booting, CHDK switches to the card's first FAT32 partition",
        "# ver_req         file that unlocks the firmware version display",
        "",
    ]
    for e in cameras:
        lines.append("[[camera]]")
        for k in FIELD_ORDER:
            v = e.get(k)
            if v is None or (k in ("aliases", "fw_check", "notes") and not v):
                continue
            val = f"0x{v:04X}" if k == "usb_pid" else toml_value(v)
            lines.append(f"{k} = {val}")
        lines.append("")
    OUT.write_text("\n".join(lines))
    print(f"wrote {len(cameras)} cameras to {OUT.relative_to(ROOT)}", file=sys.stderr)


if __name__ == "__main__":
    main()
