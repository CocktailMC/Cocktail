#!/usr/bin/env python3
"""规则与版本规划的单元测试。不访问网络。"""

from __future__ import annotations

import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

import cocktail_release as rel


def rules() -> rel.Rules:
    return rel.load_rules()


def pr(number: int, files: list[str], labels: list[str] | None = None, when: str = "2026-10-04T00:00:00Z") -> rel.PullRequest:
    return rel.PullRequest(number, f"change {number}", when, tuple(labels or []), tuple(files))


NOW = datetime(2026, 10, 4, tzinfo=timezone.utc)


class VersionTests(unittest.TestCase):
    def test_parse_dt_with_build(self) -> None:
        version = rel.parse_version("26Q4.18.AT-D.03+B1842")
        self.assertEqual(version.stage, "AT-D")
        self.assertEqual(version.major, 18)
        self.assertEqual(version.build, "B1842")
        self.assertEqual(version.base(rules()), "26Q4.18.AT-D.03")
        self.assertEqual(version.tag(rules()), "v26Q4.18.AT-D.03")

    def test_reject_cargo_transition_string(self) -> None:
        with self.assertRaises(ValueError):
            rel.parse_version("26.4.11-DP")

    def test_tag_ignores_legacy_and_build_metadata(self) -> None:
        self.assertIsNone(rel.parse_tag("2026Q4"))
        self.assertIsNone(rel.parse_tag("v26Q4.18.DT.01+B12"))
        parsed = rel.parse_tag("v26Q4.18.DT.01")
        self.assertIsNotNone(parsed)
        assert parsed is not None
        self.assertEqual(parsed.major, 18)

    def test_quarter(self) -> None:
        self.assertEqual(rel.quarter_of(datetime(2026, 1, 1, tzinfo=timezone.utc)), (26, 1))
        self.assertEqual(rel.quarter_of(NOW), (26, 4))
        self.assertEqual(rel.quarter_of(datetime(2026, 12, 31, tzinfo=timezone.utc)), (26, 4))


class CountingTests(unittest.TestCase):
    def test_docs_only_do_not_count(self) -> None:
        sample = pr(1, ["README.md", "design/webui/home.png", ".github/workflows/ci.yml"])
        self.assertFalse(rel.is_counting(sample, rules()))

    def test_code_change_counts(self) -> None:
        sample = pr(2, ["README.md", "crates/cocktail-control/src/main.rs"])
        self.assertTrue(rel.is_counting(sample, rules()))

    def test_skip_label(self) -> None:
        sample = pr(3, ["crates/cocktail-control/src/lib.rs"], ["dt:skip"])
        self.assertFalse(rel.is_counting(sample, rules()))

    def test_empty_files_do_not_count(self) -> None:
        self.assertFalse(rel.is_counting(pr(4, []), rules()))

    def test_since_excludes_already_shipped(self) -> None:
        older = pr(1, ["crates/a.rs"], when="2026-10-01T00:00:00Z")
        newer = pr(2, ["crates/b.rs"], when="2026-10-05T00:00:00Z")
        counted = rel.counting_since([older, newer], "2026-10-03T00:00:00Z", rules())
        self.assertEqual([item.number for item in counted], [2])


class DecisionTests(unittest.TestCase):
    def test_one_pr_does_not_release(self) -> None:
        decision = rel.decide_auto(
            [pr(1, ["crates/a.rs"])],
            ["2026Q4"],
            None,
            NOW,
            "B12",
            rules(),
        )
        self.assertFalse(decision.release)
        self.assertIn("1/2", decision.reason)

    def test_two_prs_cut_first_dt_of_quarter(self) -> None:
        decision = rel.decide_auto(
            [pr(1, ["crates/a.rs"]), pr(2, ["admin/src/App.tsx"])],
            ["2026Q4", "v26Q3.04.GA.01"],
            None,
            NOW,
            "B12",
            rules(),
        )
        self.assertTrue(decision.release)
        assert decision.version is not None
        self.assertEqual(decision.version.base(rules()), "26Q4.01.DT.01")
        self.assertEqual(decision.version.tag(rules()), "v26Q4.01.DT.01")
        self.assertNotIn("+", decision.version.tag(rules()))

    def test_existing_major_increments(self) -> None:
        decision = rel.decide_auto(
            [pr(8, ["crates/a.rs"]), pr(9, ["crates/b.rs"])],
            ["v26Q4.18.DT.01", "v26Q4.20.DP.01"],
            None,
            NOW,
            "B99",
            rules(),
        )
        assert decision.version is not None
        self.assertEqual(decision.version.major, 21)
        self.assertEqual(decision.version.stage, "DT")

    def test_manual_minor_bump_stays_on_same_major(self) -> None:
        decision = rel.decide_manual(
            ["v26Q4.23.RC.01"],
            NOW,
            "B3",
            rules(),
            "RC",
            "minor",
            1,
        )
        assert decision.version is not None
        self.assertEqual(decision.version.base(rules()), "26Q4.23.RC.02")
        self.assertFalse(decision.version.stage in rules().prerelease and decision.version.stage == "GA")
        self.assertIn("RC", rules().prerelease)

    def test_manual_minor_without_history_fails(self) -> None:
        with self.assertRaises(ValueError):
            rel.decide_manual([], NOW, "B3", rules(), "GA", "minor", 1)

    def test_asset_name(self) -> None:
        version = rel.parse_version("26Q4.01.DT.01")
        version = rel.Version(version.yy, version.quarter, version.major, version.stage, version.minor, "B12")
        self.assertEqual(
            rel.asset_name(version, "deb", rules()),
            "Cocktail.26Q4.01.DT.01+B12.Linux.x86_64.deb",
        )

    def test_rename_dist(self) -> None:
        version = rel.Version(26, 4, 1, "DT", 1, "B12")
        with tempfile.TemporaryDirectory() as tmp:
            dist = Path(tmp)
            (dist / "cocktail_26Q4.01.DT.01_amd64.deb").write_text("deb", encoding="utf-8")
            (dist / "cocktail.rpm").write_text("rpm", encoding="utf-8")
            (dist / "linux.tar.gz").write_text("tar", encoding="utf-8")
            renamed = rel.rename_dist(dist, version, rules(), ["deb", "rpm", "tar.gz"])
            names = sorted(path.name for path in renamed)
            self.assertEqual(
                names,
                [
                    "Cocktail.26Q4.01.DT.01+B12.Linux.x86_64.deb",
                    "Cocktail.26Q4.01.DT.01+B12.Linux.x86_64.rpm",
                    "Cocktail.26Q4.01.DT.01+B12.Linux.x86_64.tar.gz",
                ],
            )


if __name__ == "__main__":
    unittest.main()
