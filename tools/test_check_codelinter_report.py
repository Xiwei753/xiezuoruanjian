#!/usr/bin/env python3
"""test_check_codelinter_report.py — check_codelinter_report.py 的单元测试。

codelinter 的启动脚本不把规则命中透传为非零退出码，所以静态门禁必须自己
读 JSON 报告。这里覆盖报告缺失、格式错误、无 error、有 error 等分支。
"""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("check_codelinter_report.py")
SPEC = importlib.util.spec_from_file_location("check_codelinter_report", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class LoadReportTests(unittest.TestCase):
    def test_missing_file_returns_none(self) -> None:
        result = MODULE.load_report(Path("/nonexistent/codelinter-report.json"))
        self.assertIsNone(result)

    def test_invalid_json_returns_none(self) -> None:
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            f.write("not valid json {{{")
            path = Path(f.name)
        try:
            self.assertIsNone(MODULE.load_report(path))
        finally:
            path.unlink()

    def test_non_array_returns_none(self) -> None:
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump({"filePath": "x", "messages": []}, f)
            path = Path(f.name)
        try:
            self.assertIsNone(MODULE.load_report(path))
        finally:
            path.unlink()

    def test_valid_array_returns_list(self) -> None:
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump([], f)
            path = Path(f.name)
        try:
            self.assertEqual(MODULE.load_report(path), [])
        finally:
            path.unlink()


class CollectErrorsTests(unittest.TestCase):
    def test_empty_report_no_errors(self) -> None:
        self.assertEqual(MODULE.collect_errors([]), [])

    def test_no_error_severity_no_errors(self) -> None:
        report = [
            {
                "filePath": "Foo.ets",
                "messages": [
                    {"severity": "warning", "line": 1, "column": 1, "rule": "R1", "message": "w"},
                ],
            },
        ]
        self.assertEqual(MODULE.collect_errors(report), [])

    def test_error_severity_collected(self) -> None:
        report = [
            {
                "filePath": "Foo.ets",
                "messages": [
                    {"severity": "error", "line": 10, "column": 5, "rule": "arkts-no-spread", "message": "no spread"},
                ],
            },
        ]
        errors = MODULE.collect_errors(report)
        self.assertEqual(len(errors), 1)
        self.assertIn("Foo.ets:10:5", errors[0])
        self.assertIn("arkts-no-spread", errors[0])

    def test_mixed_severity_only_errors_collected(self) -> None:
        report = [
            {
                "filePath": "A.ets",
                "messages": [
                    {"severity": "warning", "line": 1, "column": 1, "rule": "Rw", "message": "w"},
                    {"severity": "error", "line": 2, "column": 1, "rule": "Re", "message": "e"},
                ],
            },
            {
                "filePath": "B.ets",
                "messages": [
                    {"severity": "Error", "line": 3, "column": 1, "rule": "Re2", "message": "e2"},
                ],
            },
        ]
        errors = MODULE.collect_errors(report)
        self.assertEqual(len(errors), 2)

    def test_non_dict_entries_skipped(self) -> None:
        report = ["not a dict", 42, None, {"filePath": "x", "messages": "not a list"}]
        self.assertEqual(MODULE.collect_errors(report), [])


class MainTests(unittest.TestCase):
    def test_wrong_arg_count_returns_2(self) -> None:
        self.assertEqual(MODULE.main(["prog"]), 2)
        self.assertEqual(MODULE.main(["prog", "a", "b"]), 2)

    def test_missing_report_returns_1(self) -> None:
        self.assertEqual(MODULE.main(["prog", "/nonexistent/report.json"]), 1)

    def test_clean_report_returns_0(self) -> None:
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump([], f)
            path = f.name
        try:
            self.assertEqual(MODULE.main(["prog", path]), 0)
        finally:
            Path(path).unlink()

    def test_report_with_errors_returns_1(self) -> None:
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump([
                {
                    "filePath": "Bad.ets",
                    "messages": [
                        {"severity": "error", "line": 1, "column": 1, "rule": "R", "message": "bad"},
                    ],
                },
            ], f)
            path = f.name
        try:
            self.assertEqual(MODULE.main(["prog", path]), 1)
        finally:
            Path(path).unlink()

    def test_invalid_json_returns_1(self) -> None:
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            f.write("not json")
            path = f.name
        try:
            self.assertEqual(MODULE.main(["prog", path]), 1)
        finally:
            Path(path).unlink()


if __name__ == "__main__":
    unittest.main()
