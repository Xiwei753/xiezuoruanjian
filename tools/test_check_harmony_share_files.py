#!/usr/bin/env python3
"""test_check_harmony_share_files.py — check_harmony_share_files.py 的单元测试。

Issue #773 评论 5846511131 第2项要求把 scopes.path 改回 `/base/files`。这条要求在
API26 上不成立，本测试把两个方向都钉死：

- 工程里真实配置（官方示例同款 `/el2/base/files`）必须通过；
- `/base/files`、重复路径、父子目录、捐献路径不在 scopes 内、捐献权限超出共享权限
  等写法必须被判失败（无论有没有 SDK schema，内置规则都要拦住）。
"""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("check_harmony_share_files.py")
SPEC = importlib.util.spec_from_file_location("check_harmony_share_files", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)

REPO_ROOT = Path(__file__).resolve().parents[1]

OFFICIAL_EXAMPLE = {
    "share_files": {
        "scopes": [
            {"path": "/el2/base/files", "permission": "r+w"},
            {"path": "/el3/distributedfiles/files/tmp", "permission": "r+w"},
        ],
        "sharingOSPath": "/el2/base/files",
        "sharingOSSubpath": "/subdir",
        "sharingOSPermission": "r",
    }
}

CURRENT_CONFIG = {
    "share_files": {
        "scopes": [{"path": "/el2/base/files", "permission": "r+w"}],
        "sharingOSPath": "/el2/base/files",
        "sharingOSSubpath": "/diagnostics",
        "sharingOSPermission": "r",
    }
}


def rules(findings: list[object]) -> list[str]:
    return [finding.rule for finding in findings]


class RepositoryProfileTests(unittest.TestCase):
    def test_tracked_profile_passes(self) -> None:
        """仓库里真正提交的 share_files.json 必须通过（含 SDK schema 交叉复核）。"""
        self.assertEqual(MODULE.check_repository(REPO_ROOT), [])

    def test_tracked_profile_is_sandbox_el_path(self) -> None:
        profile = json.loads((REPO_ROOT / MODULE.PROFILE_REL).read_text(encoding="utf-8"))
        scope_paths = [scope["path"] for scope in profile["share_files"]["scopes"]]
        self.assertEqual(scope_paths, ["/el2/base/files"])
        self.assertEqual(profile["share_files"]["sharingOSPath"], scope_paths[0])


class PathShapeTests(unittest.TestCase):
    def test_official_example_passes(self) -> None:
        self.assertEqual(MODULE.check_profile(OFFICIAL_EXAMPLE), [])

    def test_base_files_fails_builtin_rule(self) -> None:
        profile = {
            "share_files": {
                "scopes": [{"path": "/base/files", "permission": "r+w"}],
                "sharingOSPath": "/base/files",
                "sharingOSSubpath": "/diagnostics",
                "sharingOSPermission": "r",
            }
        }
        self.assertIn("path-shape", rules(MODULE.check_profile(profile)))

    def test_base_files_fails_vendor_pattern(self) -> None:
        pattern = MODULE.re.compile(
            r"^/(?:el1|el2|el3|el4|el5)/(?:base|distributedfiles|cloud)(?:/[a-zA-Z0-9_-]+){0,8}$"
        )
        findings = MODULE.check_profile(
            {"share_files": {"scopes": [{"path": "/base/files", "permission": "r+w"}]}},
            {"path": pattern, "sharingOSPath": pattern},
        )
        self.assertIn("vendor-schema", rules(findings))

    def test_too_shallow_and_too_deep(self) -> None:
        for path in ("/el2", "/el2/base/files/1/2/3/4/5/6/7/8/9"):
            with self.subTest(path=path):
                findings = MODULE.check_profile(
                    {"share_files": {"scopes": [{"path": path, "permission": "r"}]}}
                )
                self.assertIn("path-shape", rules(findings))

    def test_bad_permission_value(self) -> None:
        findings = MODULE.check_profile(
            {"share_files": {"scopes": [{"path": "/el2/base/files", "permission": "write"}]}}
        )
        self.assertIn("path-shape", rules(findings))

    def test_duplicate_paths(self) -> None:
        findings = MODULE.check_profile(
            {
                "share_files": {
                    "scopes": [
                        {"path": "/el2/base/files", "permission": "r"},
                        {"path": "/el2/base/files", "permission": "r"},
                    ]
                }
            }
        )
        self.assertIn("path-set", rules(findings))

    def test_parent_and_child_together(self) -> None:
        findings = MODULE.check_profile(
            {
                "share_files": {
                    "scopes": [
                        {"path": "/el2/base/files", "permission": "r"},
                        {"path": "/el2/base/files/diagnostics", "permission": "r"},
                    ]
                }
            }
        )
        self.assertIn("path-set", rules(findings))

    def test_too_many_scopes(self) -> None:
        findings = MODULE.check_profile(
            {
                "share_files": {
                    "scopes": [
                        {"path": f"/el2/base/files/dir{i}", "permission": "r"} for i in range(21)
                    ]
                }
            }
        )
        self.assertIn("path-set", rules(findings))

    def test_unknown_field_rejected(self) -> None:
        findings = MODULE.check_profile({"share_files": {"scopes": [], "extra": 1}})
        self.assertIn("schema-shape", rules(findings))

    def test_top_level_key_must_be_share_files(self) -> None:
        findings = MODULE.check_profile({"shareFiles": {}})
        self.assertIn("schema-shape", rules(findings))


class DonationTests(unittest.TestCase):
    def test_sharing_os_path_must_be_a_scope_path(self) -> None:
        findings = MODULE.check_profile(
            {
                "share_files": {
                    "scopes": [{"path": "/el2/base/files", "permission": "r+w"}],
                    "sharingOSPath": "/el2/base/preferences",
                    "sharingOSSubpath": "",
                    "sharingOSPermission": "r",
                }
            }
        )
        self.assertIn("scope-donation", rules(findings))

    def test_sharing_os_permission_must_be_subset(self) -> None:
        findings = MODULE.check_profile(
            {
                "share_files": {
                    "scopes": [{"path": "/el2/base/files", "permission": "r"}],
                    "sharingOSPath": "/el2/base/files",
                    "sharingOSSubpath": "",
                    "sharingOSPermission": "r+w",
                }
            }
        )
        self.assertIn("scope-donation", rules(findings))

    def test_empty_subpath_donates_scope_itself(self) -> None:
        self.assertEqual(
            MODULE.check_profile(
                {
                    "share_files": {
                        "scopes": [{"path": "/el2/base/files", "permission": "r+w"}],
                        "sharingOSPath": "/el2/base/files",
                        "sharingOSSubpath": "",
                        "sharingOSPermission": "r+w",
                    }
                }
            ),
            [],
        )

    def test_subpath_too_long(self) -> None:
        findings = MODULE.check_profile(
            {
                "share_files": {
                    "scopes": [{"path": "/el2/base/files", "permission": "r"}],
                    "sharingOSPath": "/el2/base/files",
                    "sharingOSSubpath": "/" + "a" * 40,
                    "sharingOSPermission": "r",
                }
            }
        )
        self.assertIn("scope-donation", rules(findings))

    def test_missing_subpath_when_donating(self) -> None:
        findings = MODULE.check_profile(
            {
                "share_files": {
                    "scopes": [{"path": "/el2/base/files", "permission": "r"}],
                    "sharingOSPath": "/el2/base/files",
                }
            }
        )
        self.assertIn("scope-donation", rules(findings))


class VendorSchemaTests(unittest.TestCase):
    def test_stub_schema_patterns_are_loaded_and_applied(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            schema = Path(tmp) / "shareFiles.json"
            schema.write_text(
                json.dumps(
                    {
                        "definitions": {
                            "scopeItem": {"properties": {"path": {"pattern": "^/el2/base/files$"}}},
                            "sharingOSPath": {"pattern": "^/el2/base/files$"},
                        }
                    }
                ),
                encoding="utf-8",
            )
            patterns = MODULE.load_vendor_patterns(schema)
            self.assertEqual(sorted(patterns), ["path", "sharingOSPath"])
            findings = MODULE.check_profile(
                {"share_files": {"scopes": [{"path": "/el3/base/files", "permission": "r"}]}},
                patterns,
            )
            self.assertIn("vendor-schema", rules(findings))

    def test_missing_schema_is_tolerated(self) -> None:
        self.assertEqual(MODULE.check_profile(CURRENT_CONFIG, None), [])


class RepositoryLayoutTests(unittest.TestCase):
    def _write_repo(self, tmp: str, profile: object, module_text: str) -> Path:
        root = Path(tmp)
        profile_dir = root / Path(MODULE.PROFILE_REL).parent
        profile_dir.mkdir(parents=True, exist_ok=True)
        (root / MODULE.PROFILE_REL).write_text(json.dumps(profile, indent=2), encoding="utf-8")
        (root / MODULE.MODULE_REL).write_text(module_text, encoding="utf-8")
        return root

    def test_module_json5_must_reference_profile(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = self._write_repo(tmp, CURRENT_CONFIG, '{"module": {}}')
            self.assertIn("module-reference", rules(MODULE.check_repository(root)))

    def test_module_json5_reference_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = self._write_repo(
                tmp, CURRENT_CONFIG, '{"module": {"shareFiles": "$profile:share_files"}}'
            )
            self.assertEqual(MODULE.check_repository(root), [])

    def test_missing_profile_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            self.assertIn("missing-profile", rules(MODULE.check_repository(Path(tmp))))

    def test_invalid_json_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            profile_path = root / MODULE.PROFILE_REL
            profile_path.parent.mkdir(parents=True, exist_ok=True)
            profile_path.write_text("{ not json", encoding="utf-8")
            self.assertIn("schema-shape", rules(MODULE.check_repository(root)))


if __name__ == "__main__":
    unittest.main(verbosity=2)
