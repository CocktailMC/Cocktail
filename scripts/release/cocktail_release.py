#!/usr/bin/env python3
"""Plan Cocktail versions from scripts/release/rules.toml.

自动路径只发布 DT：默认分支上每累计 2 个有效合并 PR，切一个新的主版本。
人工路径用 workflow_dispatch，可发布其余阶段，且必须显式确认。
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import tomllib
import urllib.error
import urllib.request
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

RULES_PATH = Path(__file__).with_name("rules.toml")
STAGE_PATTERN = "AT-D|BT-D|RC-D|LTS|DT|DP|AT|BT|RC|GA|HF"
VERSION_RE = re.compile(
    rf"^(?P<yy>\d{{2}})(?P<q>Q[1-4])\.(?P<major>\d+)\.(?P<stage>{STAGE_PATTERN})\.(?P<minor>\d{{2}})(?:\+(?P<build>B\d+))?$"
)
TAG_RE = re.compile(
    rf"^v(?P<yy>\d{{2}})(?P<q>Q[1-4])\.(?P<major>\d+)\.(?P<stage>{STAGE_PATTERN})\.(?P<minor>\d{{2}})$"
)


@dataclass(frozen=True)
class Rules:
    tag_prefix: str
    major_width: int
    minor_width: int
    stage_order: tuple[str, ...]
    maintenance: tuple[str, ...]
    automatic: tuple[str, ...]
    prerelease: tuple[str, ...]
    merged_prs: int
    dt_stage: str
    dt_minor: int
    skip_labels: tuple[str, ...]
    non_counting_globs: tuple[str, ...]
    name_prefix: str
    suffix: dict[str, str]
    linux_required: tuple[str, ...]
    windows_required: tuple[str, ...]
    windows_optional: tuple[str, ...]

    def known_stages(self) -> set[str]:
        return set(self.stage_order) | set(self.maintenance)


@dataclass(frozen=True)
class Version:
    yy: int
    quarter: int
    major: int
    stage: str
    minor: int
    build: str | None = None

    def base(self, rules: Rules) -> str:
        return (
            f"{self.yy:02d}Q{self.quarter}."
            f"{self.major:0{rules.major_width}d}."
            f"{self.stage}."
            f"{self.minor:0{rules.minor_width}d}"
        )

    def full(self, rules: Rules) -> str:
        text = self.base(rules)
        if self.build:
            return f"{text}+{self.build}"
        return text

    def tag(self, rules: Rules) -> str:
        return f"{rules.tag_prefix}{self.base(rules)}"


@dataclass(frozen=True)
class PullRequest:
    number: int
    title: str
    merged_at: str
    labels: tuple[str, ...]
    files: tuple[str, ...]


@dataclass(frozen=True)
class Decision:
    release: bool
    reason: str
    version: Version | None = None
    counted: tuple[PullRequest, ...] = ()


def load_rules(path: Path = RULES_PATH) -> Rules:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    version = data["version"]
    stages = data["stages"]
    dt = data["dt"]
    assets = data["assets"]
    return Rules(
        tag_prefix=version["tag_prefix"],
        major_width=int(version["major_width"]),
        minor_width=int(version["minor_width"]),
        stage_order=tuple(stages["order"]),
        maintenance=tuple(stages["maintenance"]),
        automatic=tuple(stages["automatic"]),
        prerelease=tuple(stages["prerelease"]),
        merged_prs=int(dt["merged_prs"]),
        dt_stage=dt["stage"],
        dt_minor=int(dt["minor"]),
        skip_labels=tuple(dt["skip_labels"]["labels"]),
        non_counting_globs=tuple(dt["non_counting"]["globs"]),
        name_prefix=assets["name_prefix"],
        suffix=dict(assets["suffix"]),
        linux_required=tuple(assets["linux_required"]),
        windows_required=tuple(assets["windows_required"]),
        windows_optional=tuple(assets["windows_optional"]),
    )


def parse_version(text: str) -> Version:
    match = VERSION_RE.match(text.strip())
    if not match:
        raise ValueError(
            f"不是 Cocktail 版本：{text!r}。需要 YYQn.MAJOR.STAGE.MINOR 或带 +Bxxxx"
        )
    return Version(
        yy=int(match.group("yy")),
        quarter=int(match.group("q")[1]),
        major=int(match.group("major")),
        stage=match.group("stage"),
        minor=int(match.group("minor")),
        build=match.group("build"),
    )


def parse_tag(text: str) -> Version | None:
    match = TAG_RE.match(text.strip())
    if not match:
        return None
    return Version(
        yy=int(match.group("yy")),
        quarter=int(match.group("q")[1]),
        major=int(match.group("major")),
        stage=match.group("stage"),
        minor=int(match.group("minor")),
    )


def normalize_build(raw: str) -> str:
    text = raw.strip()
    if text.startswith("B") and text[1:].isdigit():
        return text
    if text.isdigit():
        return f"B{text}"
    raise ValueError(f"构建号必须是整数或 B 加整数，收到 {raw!r}")


def quarter_of(moment: datetime) -> tuple[int, int]:
    if moment.tzinfo is None:
        moment = moment.replace(tzinfo=timezone.utc)
    moment = moment.astimezone(timezone.utc)
    return moment.year % 100, (moment.month - 1) // 3 + 1


def glob_match(path: str, pattern: str) -> bool:
    path = path.replace("\\", "/")
    while path.startswith("./"):
        path = path[2:]
    path = path.lstrip("/")
    pattern = pattern.replace("\\", "/")
    if pattern.startswith("**/"):
        rest = pattern[3:]
        if rest.startswith("*."):
            return path.endswith(rest[1:])
        return path == rest or path.endswith("/" + rest)
    if pattern.endswith("/**"):
        prefix = pattern[:-3]
        return path == prefix or path.startswith(prefix + "/")
    if pattern.startswith("*."):
        return path.endswith(pattern[1:])
    return path == pattern


def is_counting(pr: PullRequest, rules: Rules) -> bool:
    if any(label in rules.skip_labels for label in pr.labels):
        return False
    if not pr.files:
        return False
    return any(
        not any(glob_match(path, pattern) for pattern in rules.non_counting_globs)
        for path in pr.files
    )


def counting_since(
    prs: list[PullRequest], since: str | None, rules: Rules
) -> list[PullRequest]:
    selected = []
    for pr in prs:
        if since and pr.merged_at <= since:
            continue
        if is_counting(pr, rules):
            selected.append(pr)
    selected.sort(key=lambda item: (item.merged_at, item.number))
    return selected


def versions_in_quarter(tags: list[str], yy: int, quarter: int) -> list[Version]:
    found = []
    for tag in tags:
        parsed = parse_tag(tag)
        if parsed and parsed.yy == yy and parsed.quarter == quarter:
            found.append(parsed)
    return found


def next_major(tags: list[str], moment: datetime, rules: Rules, stage: str, minor: int, build: str) -> Version:
    yy, quarter = quarter_of(moment)
    existing = versions_in_quarter(tags, yy, quarter)
    major = max((item.major for item in existing), default=0) + 1
    return Version(yy, quarter, major, stage, minor, build)


def bump_minor(tags: list[str], moment: datetime, rules: Rules, stage: str, build: str) -> Version:
    yy, quarter = quarter_of(moment)
    same = [item for item in versions_in_quarter(tags, yy, quarter) if item.stage == stage]
    if not same:
        raise ValueError(f"本季度还没有 {stage}，不能只加小版本。请用 major bump。")
    current = max(same, key=lambda item: (item.major, item.minor))
    minor = current.minor + 1
    if minor > 99:
        raise ValueError("小版本超过 99。请改用 major bump。")
    return Version(yy, quarter, current.major, stage, minor, build)


def decide_auto(
    prs: list[PullRequest],
    tags: list[str],
    since: str | None,
    moment: datetime,
    build: str,
    rules: Rules,
) -> Decision:
    counted = counting_since(prs, since, rules)
    if len(counted) < rules.merged_prs:
        return Decision(
            False,
            f"有效 PR {len(counted)}/{rules.merged_prs}，不发 DT",
            counted=tuple(counted),
        )
    version = next_major(
        tags, moment, rules, rules.dt_stage, rules.dt_minor, build
    )
    numbers = ", ".join(f"#{item.number}" for item in counted)
    return Decision(
        True,
        f"有效 PR 已达到 {rules.merged_prs} 个（{numbers}），发布 {version.base(rules)}",
        version,
        tuple(counted),
    )


def decide_manual(
    tags: list[str],
    moment: datetime,
    build: str,
    rules: Rules,
    stage: str,
    bump: str,
    minor: int,
) -> Decision:
    if stage not in rules.known_stages():
        raise ValueError(f"未知阶段 {stage}")
    if bump == "minor":
        version = bump_minor(tags, moment, rules, stage, build)
    elif bump == "major":
        version = next_major(tags, moment, rules, stage, minor, build)
    else:
        raise ValueError("bump 只能是 major 或 minor")
    return Decision(True, f"人工发布 {version.base(rules)}", version)


def asset_name(version: Version, kind: str, rules: Rules) -> str:
    if kind not in rules.suffix:
        raise ValueError(f"未知制品类型 {kind}")
    if not version.build:
        raise ValueError("制品名需要构建号")
    return f"{rules.name_prefix}.{version.base(rules)}+{version.build}.{rules.suffix[kind]}"


def rename_dist(dist: Path, version: Version, rules: Rules, required: list[str]) -> list[Path]:
    patterns = {
        "deb": "*.deb",
        "rpm": "*.rpm",
        "zip": "*.zip",
        "msi": "*.msi",
        "tar.gz": "*.tar.gz",
    }
    renamed: list[Path] = []
    found: dict[str, Path] = {}
    for kind, pattern in patterns.items():
        matches = sorted(path for path in dist.glob(pattern) if path.is_file())
        if len(matches) > 1:
            names = ", ".join(path.name for path in matches)
            raise ValueError(f"{kind} 有多个文件，无法命名：{names}")
        if matches:
            found[kind] = matches[0]
    for kind in required:
        if kind not in found:
            raise ValueError(f"缺少 {kind} 制品")
    for kind, source in found.items():
        target = dist / asset_name(version, kind, rules)
        if source != target:
            source.replace(target)
        renamed.append(target)
    return renamed


def release_notes(decision: Decision, rules: Rules, commit: str) -> str:
    if not decision.release or decision.version is None:
        return decision.reason + "\n"
    version = decision.version
    lines = [
        f"版本 `{version.base(rules)}`",
        f"构建 `{version.build}`",
        f"提交 `{commit or '未知'}`",
        "",
    ]
    if decision.counted:
        lines.append("计入的 pull request：")
        lines.append("")
        for pr in decision.counted:
            lines.append(f"- #{pr.number} {pr.title}")
        lines.append("")
    else:
        lines.append("人工发布，不消耗 DT 计数。")
        lines.append("")
    lines.append("已公开的版本不会被覆盖。构建号只出现在文件名里，不进 Git tag。")
    lines.append("")
    return "\n".join(lines)


def write_github_output(path: Path, values: dict[str, str]) -> None:
    lines = []
    for key, value in values.items():
        if "\n" in value:
            fence = "COCKTAIL_RELEASE_NOTES"
            lines.append(f"{key}<<{fence}")
            lines.append(value.rstrip("\n"))
            lines.append(fence)
        else:
            lines.append(f"{key}={value}")
    with path.open("a", encoding="utf-8") as handle:
        handle.write("\n".join(lines) + "\n")


def decision_outputs(decision: Decision, rules: Rules) -> dict[str, str]:
    if not decision.release or decision.version is None:
        return {
            "release": "false",
            "reason": decision.reason,
        }
    version = decision.version
    prerelease = "true" if version.stage in rules.prerelease else "false"
    return {
        "release": "true",
        "reason": decision.reason,
        "version": version.base(rules),
        "full": version.full(rules),
        "tag": version.tag(rules),
        "build": version.build or "",
        "prerelease": prerelease,
        "title": f"Cocktail Manager {version.base(rules)}",
    }


def parse_moment(text: str | None) -> datetime:
    if not text:
        return datetime.now(timezone.utc)
    if text.endswith("Z"):
        text = text[:-1] + "+00:00"
    moment = datetime.fromisoformat(text)
    if moment.tzinfo is None:
        moment = moment.replace(tzinfo=timezone.utc)
    return moment


def _request(url: str, token: str) -> tuple[object, str]:
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "User-Agent": "cocktail-release",
            "X-GitHub-Api-Version": "2022-11-28",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            payload = json.loads(response.read().decode("utf-8"))
            return payload, response.headers.get("Link", "")
    except urllib.error.HTTPError as error:
        body = error.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"GitHub API {error.code} {url}: {body}") from error


def _next_link(link_header: str) -> str | None:
    for part in link_header.split(","):
        if 'rel="next"' not in part:
            continue
        match = re.search(r"<([^>]+)>", part)
        if match:
            return match.group(1)
    return None


def load_github(repo: str, token: str, base: str) -> tuple[list[PullRequest], list[tuple[str, str]]]:
    prs: list[PullRequest] = []
    url: str | None = (
        f"https://api.github.com/repos/{repo}/pulls?state=closed&base={base}&per_page=100"
    )
    while url:
        payload, link = _request(url, token)
        if not isinstance(payload, list):
            raise RuntimeError("pulls API 返回了意外的内容")
        for item in payload:
            merged_at = item.get("merged_at")
            if not merged_at:
                continue
            number = int(item["number"])
            files_url: str | None = item["url"] + "/files?per_page=100"
            files: list[str] = []
            while files_url:
                file_payload, file_link = _request(files_url, token)
                if not isinstance(file_payload, list):
                    raise RuntimeError(f"PR #{number} 的文件列表不可用")
                files.extend(entry["filename"] for entry in file_payload)
                files_url = _next_link(file_link)
            labels = tuple(label["name"] for label in item.get("labels") or [])
            prs.append(
                PullRequest(number, item.get("title") or "", merged_at, labels, tuple(files))
            )
        url = _next_link(link)

    tags: list[tuple[str, str]] = []
    tag_url: str | None = f"https://api.github.com/repos/{repo}/tags?per_page=100"
    while tag_url:
        payload, link = _request(tag_url, token)
        if not isinstance(payload, list):
            raise RuntimeError("tags API 返回了意外的内容")
        for item in payload:
            name = item["name"]
            if parse_tag(name) is None:
                continue
            commit_payload, _ = _request(item["commit"]["url"], token)
            if not isinstance(commit_payload, dict):
                continue
            committed = commit_payload["commit"]["committer"]["date"]
            tags.append((name, committed))
        tag_url = _next_link(link)
    return prs, tags


def window_start(tags: list[tuple[str, str]]) -> str | None:
    if not tags:
        return None
    return max(committed for _, committed in tags)


def load_fixture(path: Path) -> tuple[list[PullRequest], list[str], str | None]:
    data = json.loads(path.read_text(encoding="utf-8"))
    prs = [
        PullRequest(
            number=int(item["number"]),
            title=item.get("title") or "",
            merged_at=item["merged_at"],
            labels=tuple(item.get("labels") or []),
            files=tuple(item.get("files") or []),
        )
        for item in data.get("prs", [])
    ]
    tag_names = []
    latest = data.get("since")
    for item in data.get("tags", []):
        if isinstance(item, str):
            tag_names.append(item)
            continue
        tag_names.append(item["name"])
        committed = item.get("committed_at")
        if committed and (latest is None or committed > latest):
            latest = committed
    return prs, tag_names, latest


def plan_from_args(args: argparse.Namespace, rules: Rules) -> Decision:
    build = normalize_build(args.build_id)
    moment = parse_moment(args.now)
    if args.fixture:
        prs, tags, since = load_fixture(Path(args.fixture))
    else:
        token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
        if not token:
            raise RuntimeError("缺少 GITHUB_TOKEN，无法读取 PR 和 tag")
        prs, tagged = load_github(args.repo, token, rules_base_branch(rules))
        tags = [name for name, _ in tagged]
        since = window_start(tagged)
    if args.mode == "auto":
        return decide_auto(prs, tags, since, moment, build, rules)
    if not args.stage:
        raise ValueError("人工发布需要 --stage")
    minor = int(args.minor)
    if minor < 1 or minor > 99:
        raise ValueError("小版本必须在 1 到 99 之间")
    return decide_manual(tags, moment, build, rules, args.stage, args.bump, minor)


def rules_base_branch(rules: Rules) -> str:
    # 重新读一次，避免把分支塞进已经冻结的比较逻辑之外。
    data = tomllib.loads(RULES_PATH.read_text(encoding="utf-8"))
    return data["dt"]["base_branch"]


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Cocktail 版本规划")
    sub = parser.add_subparsers(dest="command", required=True)

    validate = sub.add_parser("validate", help="检查一个版本字符串")
    validate.add_argument("version")

    plan = sub.add_parser("plan", help="决定这次要不要发版")
    plan.add_argument("--mode", choices=("auto", "manual"), default="auto")
    plan.add_argument("--build-id", required=True)
    plan.add_argument("--repo", default="CocktailMC/Cocktail")
    plan.add_argument("--now")
    plan.add_argument("--fixture")
    plan.add_argument("--stage")
    plan.add_argument("--bump", choices=("major", "minor"), default="major")
    plan.add_argument("--minor", default="1")
    plan.add_argument("--commit", default=os.environ.get("GITHUB_SHA", ""))
    plan.add_argument("--github-output")
    plan.add_argument("--notes-file")

    asset = sub.add_parser("asset-name", help="打印规范制品文件名")
    asset.add_argument("--version", required=True)
    asset.add_argument("--build", required=True)
    asset.add_argument("--kind", required=True)

    rename = sub.add_parser("rename-dist", help="把 dist 里的包改成规范文件名")
    rename.add_argument("--version", required=True)
    rename.add_argument("--build", required=True)
    rename.add_argument("--dist", type=Path, required=True)
    rename.add_argument("--require", required=True, help="逗号分隔，例如 deb,rpm,tar.gz")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    rules = load_rules()
    try:
        if args.command == "validate":
            version = parse_version(args.version)
            if version.stage not in rules.known_stages():
                raise ValueError(f"阶段 {version.stage} 不在规则表里")
            print(version.full(rules))
            return 0
        if args.command == "asset-name":
            version = parse_version(args.version)
            version = Version(
                version.yy,
                version.quarter,
                version.major,
                version.stage,
                version.minor,
                normalize_build(args.build),
            )
            print(asset_name(version, args.kind, rules))
            return 0
        if args.command == "rename-dist":
            version = parse_version(args.version)
            version = Version(
                version.yy,
                version.quarter,
                version.major,
                version.stage,
                version.minor,
                normalize_build(args.build),
            )
            required = [item.strip() for item in args.require.split(",") if item.strip()]
            paths = rename_dist(args.dist, version, rules, required)
            for path in paths:
                print(path)
            return 0
        decision = plan_from_args(args, rules)
        payload = decision_outputs(decision, rules)
        if args.notes_file:
            notes_path = Path(args.notes_file)
            notes_path.parent.mkdir(parents=True, exist_ok=True)
            notes_path.write_text(release_notes(decision, rules, args.commit), encoding="utf-8")
        print(json.dumps(payload, ensure_ascii=False, indent=2))
        if args.github_output:
            write_github_output(Path(args.github_output), payload)
        return 0
    except (ValueError, RuntimeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
