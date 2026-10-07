#!/usr/bin/env python3
import importlib.util
import unittest
from unittest import mock
from pathlib import Path

SCRIPT = Path(__file__).with_name("publish_harmony_test.py")
SPEC = importlib.util.spec_from_file_location("publish_harmony_test", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class PublishHarmonyTestHelpers(unittest.TestCase):
    def test_resolve_hvigorw_uses_cli_path(self) -> None:
        with mock.patch.object(
            MODULE.shutil,
            "which",
            return_value="/home/runner/.harmony-cli/bin/hvigorw",
        ):
            self.assertEqual(
                "/home/runner/.harmony-cli/bin/hvigorw",
                MODULE.resolve_hvigorw(),
            )

    def test_resolve_hvigorw_requires_cli_path(self) -> None:
        with mock.patch.object(MODULE.shutil, "which", return_value=None):
            with self.assertRaises(MODULE.PublishError):
                MODULE.resolve_hvigorw()

    def test_parse_last_json_prefers_outer_response(self) -> None:
        data = MODULE.parse_last_json(
            'progress\\n{"ret":{"code":0,"msg":"success"},"versionId":"v1"}\\n'
        )
        self.assertEqual("v1", data["versionId"])

    def test_validate_business_result_rejects_nonzero_ret(self) -> None:
        with self.assertRaises(MODULE.PublishError):
            MODULE.validate_business_result(
                {"ret": {"code": 7, "msg": "bad"}},
                "demo",
            )

    def test_collect_groups_deduplicates_nested_results(self) -> None:
        groups = MODULE.collect_groups(
            {
                "data": {
                    "groups": [
                        {"groupId": "g1", "groupName": "Alpha"},
                        {"groupId": "g1", "groupName": "Alpha"},
                        {"groupId": "g2", "name": "Beta"},
                    ]
                }
            }
        )
        self.assertEqual([("g1", "Alpha"), ("g2", "Beta")], groups)

    def test_collect_test_versions_filters_commercial_and_deduplicates(self) -> None:
        versions = MODULE.collect_test_versions(
            {
                "data": [
                    {"versionId": "commercial", "releaseType": 1, "state": 2},
                    {"versionId": "test-1", "releaseType": 6, "state": 3},
                    {"versionId": "test-1", "releaseType": 6, "state": 3},
                    {"versionId": "test-2", "testType": 3, "status": "review"},
                ]
            }
        )
        self.assertEqual([("test-1", "3"), ("test-2", "review")], versions)

    def test_cleanup_old_versions_uses_cancel_stop_delete_sequence(self) -> None:
        calls = []

        class FakeCli:
            def raw(self, *args):
                calls.append(args)
                return {
                    "versions": [
                        {"versionId": "old-1", "releaseType": 6, "state": "review"}
                    ]
                }

            def try_raw(self, *args):
                calls.append(args)
                return True, {"ret": {"code": 0}}

        removed = MODULE.cleanup_old_test_versions(
            FakeCli(),
            app_id="app-1",
            package_name="com.xiwei.sujian",
        )
        self.assertEqual(["old-1"], removed)
        self.assertEqual(
            [
                (
                    "publish",
                    "version-list",
                    "-a",
                    "app-1",
                    "-p",
                    "com.xiwei.sujian",
                ),
                ("publish", "cancel-review", "-a", "app-1", "-v", "old-1"),
                ("test", "version-stop", "-a", "app-1", "-v", "old-1"),
                ("test", "version-delete", "-a", "app-1", "-v", "old-1"),
            ],
            calls,
        )

    def test_invite_update_body_binds_package_and_group(self) -> None:
        body = MODULE.build_version_update_body(
            version_id="version-1",
            pkg_id="pkg-1",
            test_desc="自动测试",
            test_type=3,
            group_id="group-1",
            start_time_ms=None,
            end_time_ms=None,
            notify=False,
        )
        self.assertEqual("version-1", body["versionId"])
        self.assertEqual("pkg-1", body["pkgId"])
        self.assertEqual(
            [{"groupId": "group-1"}],
            body["openTestInfo"]["testTaskInfo"]["groupInfos"],
        )
        self.assertEqual(
            0,
            body["openTestInfo"]["testTaskInfo"]["needNotify"],
        )

    def test_invite_update_body_requires_group(self) -> None:
        with self.assertRaises(MODULE.PublishError):
            MODULE.build_version_update_body(
                version_id="version-1",
                pkg_id="pkg-1",
                test_desc="自动测试",
                test_type=3,
                group_id=None,
                start_time_ms=None,
                end_time_ms=None,
                notify=False,
            )


if __name__ == "__main__":
    unittest.main()
