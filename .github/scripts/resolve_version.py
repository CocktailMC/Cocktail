#!/usr/bin/env python3
import os
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CARGO_PATH = ROOT / "Cargo.toml"

STAGE_VALUES = {
    "DT", "DP", "AT-D", "AT", "BT-D", "BT", "RC-D", "RC", "GA", "HF", "LTS"
}


def read_workspace_version():
    text = CARGO_PATH.read_text(encoding="utf-8")
    m = re.search(r'(?ms)^\[workspace\.package\]\s*.*?^version\s*=\s*"([^"]+)"', text)
    if not m:
        raise RuntimeError("Cannot parse [workspace.package].version from Cargo.toml")
    return m.group(1)


def parse_cargo_version(raw):
    # supports examples: 26.4.11-DP, 26.4.11-AT-D
    m = re.match(r"^(?P<yy>\d+)\.(?P<quarter>\d+)\.(?P<major>\d+)-(?P<stage>[A-Za-z0-9-]+)$", raw)
    if not m:
        raise RuntimeError(f"Unsupported Cargo version format: {raw}")
    yy = int(m.group("yy"))
    q = 4
    major = int(m.group("major"))
    stage = m.group("stage")
    if stage.upper() not in STAGE_VALUES:
        stage = "DP"
    return yy, q, major, stage


def canonical_version(year, quarter, major, stage, minor, build_num):
    return f"{str(year)[-2:]}Q{quarter}.{major}.{stage}.{minor}+B{build_num}"


def main():
    cargo_version = read_workspace_version()
    yy, q, major, stage = parse_cargo_version(cargo_version)

    force_stage = os.environ.get("FORCE_STAGE", "").strip()
    if force_stage:
        stage = force_stage

    if stage.upper() not in STAGE_VALUES:
        raise RuntimeError(f"Illegal stage: {stage}")

    force_major = os.environ.get("FORCE_MAJOR", "").strip()
    if force_major:
        major = int(force_major)

    force_minor = os.environ.get("FORCE_MINOR", "").strip()
    if not force_minor:
        force_minor = "01"
    minor = force_minor.zfill(2)

    build_num = os.environ.get("GITHUB_RUN_NUMBER", "0")
    version = canonical_version(yy, q, major, stage, minor, build_num)
    tag = f"v{version.split('+')[0]}"
    is_ga = "true" if stage.upper() == "GA" else "false"

    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as out:
        out.write(f"version={version}\n")
        out.write(f"tag={tag}\n")
        out.write(f"stage={stage}\n")
        out.write(f"major={major}\n")
        out.write(f"minor={minor}\n")
        out.write(f"build=B{build_num}\n")
        out.write(f"is_ga={is_ga}\n")

    print(f"Cargo version: {cargo_version}")
    print(f"Generated version: {version}")
    print(f"Tag: {tag}")


if __name__ == "__main__":
    main()
