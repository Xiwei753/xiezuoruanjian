#!/usr/bin/env python3
import importlib.util
import tempfile
import unittest
from unittest import mock
from pathlib import Path

SCRIPT = Path(__file__).with_name("publish_harmony_test.py")
SPEC = importlib.util.spec_from_file_location("publish_harmony_test", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class PublishHarmonyTestHelpers(unittest.TestCase):
    def test_disable_hvigor_signing_detaches_product(self) -> None:
        source = "signingConfigs: [{ name: 'default' }], products: [{ signingConfig: 'default' }]"
        updated = MODULE.disable_hvigor_signing_text(source)
        self.assertIn("signingConfig: ''", updated)
        self.assertIn("signingConfigs:", updated)

    def test_resolve_direct_signing_accepts_complete_ci_material(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            p12 = root / "release.p12"
            cer = root / "release.cer"
            profile = root / "release.p7b"
            for path in (p12, cer, profile):
                path.write_bytes(b"x")
            env = {
                "HARMONY_SIGN_P12_FILE": str(p12),
                "HARMONY_SIGN_CER_FILE": str(cer),
                "HARMONY_SIGN_PROFILE_FILE": str(profile),
                "HARMONY_SIGN_STORE_PASSWORD": "store-secret",
                "HARMONY_SIGN_KEY_ALIAS": "release-key",
                "HARMONY_SIGN_KEY_PASSWORD": "key-secret",
            }
            with mock.patch.dict(MODULE.os.environ, env, clear=True):
                signing = MODULE.resolve_direct_signing()
            self.assertIsNotNone(signing)
            assert signing is not None
            self.assertEqual(str(p12.resolve()), signing["p12"])
            self.assertEqual("release-key", signing["key_alias"])

    def test_resolve_direct_signing_rejects_partial_ci_material(self) -> None:
        with mock.patch.dict(
            MODULE.os.environ,
            {"HARMONY_SIGN_P12_FILE": "/tmp/release.p12"},
            clear=True,
        ):
            with self.assertRaises(MODULE.PublishError):
                MODULE.resolve_direct_signing()

    def test_patch_version_code_text_updates_only_version_code(self) -> None:
        source = '{"app":{"versionCode":1000000,"versionName":"1.0.0"}}'
        updated = MODULE.patch_version_code_text(source, 2_000_321)
        self.assertIn('"versionCode":2000321', updated)
        self.assertIn('"versionName":"1.0.0"', updated)

    def test_resolve_ci_version_code_uses_github_run_identity(self) -> None:
        with mock.patch.dict(
            MODULE.os.environ,
            {
                "GITHUB_ACTIONS": "true",
                "GITHUB_RUN_NUMBER": "123",
                "GITHUB_RUN_ATTEMPT": "2",
            },
            clear=True,
        ):
            self.assertEqual(2_001_232, MODULE.resolve_ci_version_code())

    def test_remove_request_permission_text_removes_only_dlp_acl(self) -> None:
        source = '''{
          "module": {
            "requestPermissions": [
              {
                "name": "ohos.permission.DETECT_GESTURE"
              },
              {
                "name": "ohos.permission.DLP_GET_HIDE_STATUS",
                "reason": "$string:perm_reason_dlp_anti_peep",
                "usedScene": {
                  "abilities": ["EntryAbility"],
                  "when": "always"
                }
              }
            ]
          }
        }'''
        updated = MODULE.remove_request_permission_text(
            source,
            "ohos.permission.DLP_GET_HIDE_STATUS",
        )
        self.assertNotIn("DLP_GET_HIDE_STATUS", updated)
        self.assertIn("DETECT_GESTURE", updated)

    def test_ci_should_include_dlp_acl_is_explicit_opt_in(self) -> None:
        with mock.patch.dict(
            MODULE.os.environ,
            {"HARMONY_RELEASE_DLP_ACL": "1"},
            clear=True,
        ):
            self.assertTrue(MODULE.ci_should_include_dlp_acl())
        with mock.patch.dict(MODULE.os.environ, {}, clear=True):
            self.assertFalse(MODULE.ci_should_include_dlp_acl())

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

    def test_find_version_package_id_reads_nested_package(self) -> None:
        data = {
            "versions": [
                {
                    "versionId": "v1",
                    "package": {"pkgId": "pkg-123"},
                }
            ]
        }
        self.assertEqual(
            "pkg-123",
            MODULE.find_version_package_id(data, "v1"),
        )

    def test_normalize_test_window_defaults_to_30_days(self) -> None:
        start, end = MODULE.normalize_test_window(
            None,
            None,
            now_ms=1_000_000,
        )
        self.assertEqual(1_000_000, start)
        self.assertEqual(
            1_000_000 + 30 * 24 * 60 * 60 * 1000,
            end,
        )

    def test_normalize_test_window_rejects_over_90_days(self) -> None:
        with self.assertRaises(MODULE.PublishError):
            MODULE.normalize_test_window(
                0,
                91 * 24 * 60 * 60 * 1000,
                now_ms=0,
            )

    def test_first_list_value_reads_pkg_version(self) -> None:
        data = {"ret": {"code": 0}, "pkgVersion": ["pkg-123"]}
        self.assertEqual(
            "pkg-123",
            MODULE.first_list_value(data, ("pkgVersion",)),
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

    def test_wait_for_package_compile_stops_on_success(self) -> None:
        calls = []

        class FakeCli:
            def raw(self, *args):
                calls.append(args)
                return {"ret": {"code": 0}, "pkgStateList": [{"successStatus": 0}]}

        MODULE.wait_for_package_compile(
            FakeCli(),
            app_id="app-1",
            pkg_id="pkg-1",
            attempts=1,
            delay_seconds=0,
        )
        self.assertEqual(
            [("publish", "compile-status", "-a", "app-1", "--pkg-ids", "pkg-1")],
            calls,
        )

    def test_wait_for_package_compile_fails_fast_on_status_2(self) -> None:
        class FakeCli:
            def raw(self, *args):
                return {"ret": {"code": 0}, "pkgStateList": [{"successStatus": 2}]}

            def try_raw(self, *args):
                return False, None

        with self.assertRaises(MODULE.PublishError):
            MODULE.wait_for_package_compile(
                FakeCli(),
                app_id="app-1",
                pkg_id="pkg-1",
                attempts=30,
                delay_seconds=0,
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

    def test_version_update_body_allows_status_only_pkg_add(self) -> None:
        body = MODULE.build_version_update_body(
            version_id="version-1",
            pkg_id=None,
            test_desc="自动测试",
            test_type=3,
            group_id="group-1",
            start_time_ms=None,
            end_time_ms=None,
            notify=False,
        )
        self.assertEqual("version-1", body["versionId"])
        self.assertNotIn("pkgId", body)
        self.assertEqual(
            [{"groupId": "group-1"}],
            body["openTestInfo"]["testTaskInfo"]["groupInfos"],
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
