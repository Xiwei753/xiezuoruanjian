#!/usr/bin/env python3
import importlib.util
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("publish_harmony_test.py")
SPEC = importlib.util.spec_from_file_location("publish_harmony_test", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class PublishHarmonyTestHelpers(unittest.TestCase):
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
