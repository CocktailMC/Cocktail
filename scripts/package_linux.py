#!/usr/bin/env python3
"""Build Linux DEB/RPM packages for Cocktail Manager.

The script intentionally keeps the package definition in packaging/nfpm.yaml so
the Debian and RPM payloads stay identical.  It only needs Python 3.11+, Cargo,
Node.js/npm (unless --skip-web is used), and nfpm.  If nfpm is missing, a pinned
copy is downloaded into dist/tools/.
"""

from __future__ import annotations

import argparse
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - guarded for clearer CLI errors
    tomllib = None


ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / "dist"
STAGE = DIST / "stage"
NFPM_VERSION = "2.41.3"

ARCHES = {
    "x86_64": ("amd64", "x86_64", "x86_64"),
    "amd64": ("amd64", "x86_64", "x86_64"),
    "aarch64": ("arm64", "aarch64", "arm64"),
    "arm64": ("arm64", "aarch64", "arm64"),
}


def run(command: list[str], *, cwd: Path = ROOT) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=cwd, check=True)


def workspace_version() -> str:
    if tomllib is None:
        raise RuntimeError("Python 3.11 or newer is required (missing tomllib).")
    with (ROOT / "Cargo.toml").open("rb") as source:
        manifest = tomllib.load(source)
    try:
        return manifest["workspace"]["package"]["version"]
    except KeyError as error:
        raise RuntimeError("workspace.package.version is missing from Cargo.toml") from error


def package_architecture(raw_arch: str) -> tuple[str, str, str]:
    normalized = raw_arch.lower()
    if normalized in ARCHES:
        return ARCHES[normalized]
    return normalized, normalized, normalized


def executable(name: str) -> str | None:
    return shutil.which(name)


def safe_extract(archive: tarfile.TarFile, destination: Path) -> None:
    destination = destination.resolve()
    for member in archive.getmembers():
        target = (destination / member.name).resolve()
        if not target.is_relative_to(destination):
            raise RuntimeError(f"refusing unsafe archive member: {member.name}")
        if not (member.isdir() or member.isreg()):
            raise RuntimeError(f"refusing non-file archive member: {member.name}")
    archive.extractall(destination)


def ensure_nfpm(nfpm_override: str | None, nfpm_arch: str) -> str:
    if nfpm_override:
        candidate = Path(nfpm_override)
        if not candidate.is_file():
            raise RuntimeError(f"nfpm executable does not exist: {candidate}")
        return str(candidate)

    if found := executable("nfpm"):
        return found

    tools = DIST / "tools"
    binary = tools / "nfpm"
    if binary.is_file():
        return str(binary)

    tools.mkdir(parents=True, exist_ok=True)
    url = (
        "https://github.com/goreleaser/nfpm/releases/download/"
        f"v{NFPM_VERSION}/nfpm_{NFPM_VERSION}_Linux_{nfpm_arch}.tar.gz"
    )
    print(f"==> nfpm not found; downloading {url}", flush=True)
    with tempfile.NamedTemporaryFile(suffix=".tar.gz") as download:
        with urllib.request.urlopen(url) as response:
            shutil.copyfileobj(response, download)
        download.flush()
        with tarfile.open(download.name, "r:gz") as archive:
            safe_extract(archive, tools)
    binary.chmod(0o755)
    return str(binary)


def render_nfpm_config(version: str, architecture: str) -> Path:
    template = (ROOT / "packaging" / "nfpm.yaml").read_text(encoding="utf-8")
    rendered = template.replace("${VERSION}", version).replace("${ARCH}", architecture)
    temporary = tempfile.NamedTemporaryFile(
        mode="w", suffix=".yaml", prefix="cocktail-nfpm-", delete=False, encoding="utf-8"
    )
    with temporary:
        temporary.write(rendered)
    return Path(temporary.name)


def clean_stage_and_packages() -> None:
    shutil.rmtree(STAGE, ignore_errors=True)
    for pattern in ("cocktail_*.deb", "cocktail-*.rpm"):
        for artifact in DIST.glob(pattern):
            artifact.unlink()


def stage_files(skip_web: bool) -> None:
    binary = ROOT / "target" / "release" / "cocktail-control"
    if not binary.is_file():
        raise RuntimeError(f"release binary is missing: {binary}")

    bin_dir = STAGE / "usr" / "bin"
    web_dir = STAGE / "usr" / "share" / "cocktail" / "web"
    bin_dir.mkdir(parents=True)
    shutil.copy2(binary, bin_dir / binary.name)
    (bin_dir / binary.name).chmod(0o755)

    source_web = ROOT / "admin" / "dist"
    if skip_web and not source_web.is_dir():
        raise RuntimeError("admin/dist is missing; omit --skip-web to build it.")
    if not source_web.is_dir():
        raise RuntimeError(f"admin build output is missing: {source_web}")
    shutil.copytree(source_web, web_dir)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--format", choices=("all", "deb", "rpm"), default="all")
    parser.add_argument("--version", help="override the version from Cargo.toml")
    parser.add_argument("--arch", default=platform.machine(), help="target architecture (default: host)")
    parser.add_argument("--skip-build", action="store_true", help="reuse target/release/cocktail-control")
    parser.add_argument("--skip-web", action="store_true", help="reuse admin/dist instead of running npm")
    parser.add_argument("--nfpm", help="path to an existing nfpm executable")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    version = args.version or workspace_version()
    deb_arch, rpm_arch, nfpm_arch = package_architecture(args.arch)
    print(f"==> version={version} deb_arch={deb_arch} rpm_arch={rpm_arch}")

    if not args.skip_build:
        run(["cargo", "build", "-p", "cocktail-control", "--release", "--bin", "cocktail-control"])
    if not args.skip_web:
        run(["npm", "ci"], cwd=ROOT / "admin")
        run(["npm", "run", "build"], cwd=ROOT / "admin")

    clean_stage_and_packages()
    stage_files(args.skip_web)
    nfpm = ensure_nfpm(args.nfpm, nfpm_arch)
    DIST.mkdir(exist_ok=True)

    formats = ("deb", "rpm") if args.format == "all" else (args.format,)
    for package_format in formats:
        architecture = deb_arch if package_format == "deb" else rpm_arch
        config = render_nfpm_config(version, architecture)
        try:
            run([nfpm, "package", "--config", str(config), "--packager", package_format, "--target", str(DIST)])
        finally:
            config.unlink(missing_ok=True)

    artifacts = sorted(
        artifact for extension in formats for artifact in DIST.glob(f"*.{extension}")
    )
    print("==> packages:")
    for artifact in artifacts:
        print(artifact.relative_to(ROOT))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RuntimeError, subprocess.CalledProcessError, urllib.error.URLError, tarfile.TarError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
