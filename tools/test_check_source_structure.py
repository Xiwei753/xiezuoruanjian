#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("check_source_structure.py")
SPEC = importlib.util.spec_from_file_location("check_source_structure", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class SourceStructureTests(unittest.TestCase):
    def rule_names(self, source: str, path: str = "sample.rs") -> set[str]:
        return {
            finding.rule
            for finding in MODULE.scan_text(Path(path), source)
        }

    # ------------------------------------------------------------------
    # 正测试：合规文件不报违规
    # ------------------------------------------------------------------

    def test_compliant_rust_file_has_no_findings(self) -> None:
        source = """\
fn main() {
    let workspace_name = "sujian";
    let chapter_count = 42;
    println!("{} has {} chapters", workspace_name, chapter_count);
}
"""
        self.assertEqual(self.rule_names(source), set())

    def test_compliant_kotlin_file_has_no_findings(self) -> None:
        source = """\
package com.xiwei.sujian

class EditorController {
    private val documentTitle: String = ""
    private val revisionCount: Int = 0

    fun save(): Boolean {
        return true
    }
}
"""
        self.assertEqual(self.rule_names(source, path="EditorController.kt"), set())

    def test_file_under_800_effective_lines_passes(self) -> None:
        """799 有效行的文件不报 god-file。"""
        lines = ["fn line_%d() {}" % i for i in range(799)]
        source = "\n".join(lines)
        self.assertNotIn("god-file", self.rule_names(source))

    def test_comments_and_blanks_not_counted_as_effective(self) -> None:
        """注释行和空行不计入有效行数。"""
        comment_lines = ["// comment line %d" % i for i in range(900)]
        code_lines = ["fn real_%d() {}" % i for i in range(10)]
        source = "\n".join(comment_lines + code_lines)
        self.assertNotIn("god-file", self.rule_names(source))

    def test_block_comments_not_counted_as_effective(self) -> None:
        """块注释行不计入有效行数。"""
        source = "/*\n" + " * block comment\n" * 900 + " */\nfn real() {}"
        self.assertNotIn("god-file", self.rule_names(source))

    def test_single_large_class_does_not_trigger_multiple_large_state(self) -> None:
        """仅一个超过 200 行的类不报 multiple-large-state。"""
        body = "    let value = 0;\n" * 200
        source = "struct BigClass {\n" + body + "}\n"
        self.assertNotIn("multiple-large-state", self.rule_names(source))

    def test_small_test_module_in_production_passes(self) -> None:
        """不超过 100 行的内嵌测试模块不报违规。"""
        test_body = "    #[test]\n    fn test_%d() {}\n" % 0
        for i in range(1, 20):
            test_body += "    #[test]\n    fn test_%d() {}\n" % i
        source = "#[cfg(test)]\nmod tests {\n" + test_body + "}\n"
        self.assertNotIn("production-test-bloat", self.rule_names(source))

    def test_meaningful_variable_names_pass(self) -> None:
        """有意义的变量名不报 short-variable-names。"""
        source = """\
fn process() {
    let workspace_path = "/tmp/ws";
    let chapter_index = 0;
    let revision_number = 1;
    let document_content = "hello";
    let sync_timestamp = 12345;
    let editor_session_id = 67890;
}
"""
        self.assertNotIn("short-variable-names", self.rule_names(source))

    def test_underscore_placeholder_does_not_trigger_short_names(self) -> None:
        """下划线占位 _ 不触发短变量名规则。"""
        source = """\
fn process() {
    let _ = compute();
    let _ = compute();
    let _ = compute();
    let _ = compute();
    let _ = compute();
    let _ = compute();
}
"""
        self.assertNotIn("short-variable-names", self.rule_names(source))

    # ------------------------------------------------------------------
    # 反测试：真实违规样例能被抓到
    # ------------------------------------------------------------------

    def test_detects_god_file_over_800_lines(self) -> None:
        """超过 800 有效行的文件报 god-file。"""
        lines = ["fn line_%d() {}" % i for i in range(801)]
        source = "\n".join(lines)
        self.assertIn("god-file", self.rule_names(source))

    def test_detects_multiple_large_state_classes(self) -> None:
        """两个超过 200 行的类报 multiple-large-state。"""
        body = "".join("    field_%d: i32,\n" % i for i in range(250))
        cls1 = "struct FirstHolder {\n" + body + "}\n"
        cls2 = "struct SecondHolder {\n" + body + "}\n"
        source = cls1 + "\n" + cls2
        rules = self.rule_names(source)
        self.assertIn("multiple-large-state", rules)

    def test_detects_production_test_bloat(self) -> None:
        """内嵌超过 100 行的测试模块报 production-test-bloat。"""
        test_body = ""
        for i in range(120):
            test_body += "    #[test]\n    fn test_%d() { assert!(true); }\n" % i
        source = "#[cfg(test)]\nmod tests {\n" + test_body + "}\n"
        self.assertIn("production-test-bloat", self.rule_names(source))

    def test_detects_rust_crate_wide_suppression(self) -> None:
        """Rust crate 级 allow(warnings) 报 broad-suppression。"""
        source = '#![allow(warnings)]\nfn main() {}\n'
        self.assertIn("broad-suppression", self.rule_names(source))

    def test_detects_rust_crate_dead_code_suppression(self) -> None:
        """Rust crate 级 allow(dead_code) 报 broad-suppression。"""
        source = '#![allow(dead_code)]\nfn main() {}\n'
        self.assertIn("broad-suppression", self.rule_names(source))

    def test_detects_rust_module_wide_suppression(self) -> None:
        """Rust 模块级 allow(warnings) 报 broad-suppression。"""
        source = '#[allow(warnings)]\nfn main() {}\n'
        self.assertIn("broad-suppression", self.rule_names(source))

    def test_detects_kotlin_file_suppress_all(self) -> None:
        """Kotlin @file:Suppress("ALL") 报 broad-suppression。"""
        source = '@file:Suppress("ALL")\npackage com.example\n\nclass Foo {}\n'
        self.assertIn(
            "broad-suppression",
            self.rule_names(source, path="Foo.kt"),
        )

    def test_detects_kotlin_file_suppress_multi(self) -> None:
        """Kotlin @file:Suppress 批量关闭多个 lint 报 broad-suppression。"""
        source = (
            '@file:Suppress("UNCHECKED_CAST", "UNUSED_PARAMETER")\n'
            'package com.example\n\nclass Foo {}\n'
        )
        self.assertIn(
            "broad-suppression",
            self.rule_names(source, path="Foo.kt"),
        )

    def test_detects_short_variable_names_rust(self) -> None:
        """Rust 连续 6 个短变量名报 short-variable-names。"""
        source = """\
fn bad() {
    let a = 1;
    let b = 2;
    let c = 3;
    let d = 4;
    let e = 5;
    let f = 6;
}
"""
        self.assertIn("short-variable-names", self.rule_names(source))

    def test_detects_short_variable_names_kotlin(self) -> None:
        """Kotlin 连续 6 个短变量名报 short-variable-names。"""
        source = """\
fun bad() {
    val a = 1
    val b = 2
    val c = 3
    val d = 4
    val e = 5
    val f = 6
}
"""
        self.assertIn(
            "short-variable-names",
            self.rule_names(source, path="Bad.kt"),
        )

    def test_detects_short_variable_names_with_generic_names(self) -> None:
        """tmp/data/result 等无语义命名也报 short-variable-names。"""
        source = """\
fn bad() {
    let tmp = 1;
    let data = 2;
    let result = 3;
    let res = 4;
    let ret = 5;
    let foo = 6;
}
"""
        self.assertIn("short-variable-names", self.rule_names(source))

    def test_short_names_reset_by_meaningful_name(self) -> None:
        """有意义的变量名会重置连续计数。"""
        source = """\
fn mixed() {
    let a = 1;
    let b = 2;
    let c = 3;
    let meaningful_name = 4;
    let d = 5;
    let e = 6;
    let f = 7;
}
"""
        self.assertNotIn("short-variable-names", self.rule_names(source))

    # ------------------------------------------------------------------
    # 生成目录排除测试
    # ------------------------------------------------------------------

    def test_repository_scan_skips_generated_directories(self) -> None:
        """生成目录下的违规文件不被扫描。"""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "bindings").mkdir()
            (root / "target").mkdir()
            (root / "apps" / "android" / "app" / "build" / "generated").mkdir(
                parents=True
            )
            (root / "src" / "good.rs").write_text(
                "fn ok() {}\n", encoding="utf-8"
            )
            # bindings 下的违规文件应被跳过
            (root / "bindings" / "bad.rs").write_text(
                "#![allow(warnings)]\nfn bad() {}\n",
                encoding="utf-8",
            )
            # target 下的违规文件应被跳过
            (root / "target" / "bad.rs").write_text(
                "#![allow(dead_code)]\nfn bad() {}\n",
                encoding="utf-8",
            )
            # build/generated 下的违规文件应被跳过
            (
                root / "apps" / "android" / "app" / "build" / "generated" / "bad.rs"
            ).write_text(
                "#![allow(warnings)]\nfn bad() {}\n",
                encoding="utf-8",
            )
            self.assertEqual(MODULE.scan_repository(root), [])

    def test_repository_scan_skips_test_directories(self) -> None:
        """测试目录和测试文件不被扫描。"""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "src" / "tests").mkdir()
            (root / "apps" / "android" / "app" / "src" / "test").mkdir(
                parents=True
            )
            (root / "src" / "good.rs").write_text(
                "fn ok() {}\n", encoding="utf-8"
            )
            # tests/ 目录下的超长文件应被跳过
            big = "fn line_%d() {}\n" * 801
            (root / "src" / "tests" / "big.rs").write_text(big, encoding="utf-8")
            # test/ 路径下的超长文件应被跳过
            (
                root / "apps" / "android" / "app" / "src" / "test" / "Big.kt"
            ).write_text("fun line() {}\n" * 801, encoding="utf-8")
            self.assertEqual(MODULE.scan_repository(root), [])

    def test_repository_scan_skips_test_suffix_files(self) -> None:
        """_tests.rs / Test.kt 后缀的文件不被扫描。"""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "src" / "good.rs").write_text(
                "fn ok() {}\n", encoding="utf-8"
            )
            big = "fn line_%d() {}\n" * 801
            (root / "src" / "foo_tests.rs").write_text(big, encoding="utf-8")
            (root / "src" / "tests.rs").write_text(big, encoding="utf-8")
            (root / "src" / "FooTest.kt").write_text(
                "fun line() {}\n" * 801, encoding="utf-8"
            )
            self.assertEqual(MODULE.scan_repository(root), [])

    def test_repository_scan_skips_build_scripts(self) -> None:
        """build.gradle.kts 等构建脚本不被扫描。"""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "src" / "good.rs").write_text(
                "fn ok() {}\n", encoding="utf-8"
            )
            (root / "build.gradle.kts").write_text(
                "// " + "x" * 1000 + "\n", encoding="utf-8"
            )
            self.assertEqual(MODULE.scan_repository(root), [])

    # ------------------------------------------------------------------
    # 白名单机制测试
    # ------------------------------------------------------------------

    def test_whitelist_entry_without_reason_is_flagged(self) -> None:
        """白名单条目缺少原因会报 no-reason-exception。"""
        original = dict(MODULE.ALLOWED_EXCEPTIONS)
        try:
            MODULE.ALLOWED_EXCEPTIONS[
                (Path("some/file.rs"), "god-file")
            ] = ""
            findings = MODULE.scan_repository(Path("/nonexistent"))
            rules = {f.rule for f in findings}
            self.assertIn("no-reason-exception", rules)
        finally:
            MODULE.ALLOWED_EXCEPTIONS.clear()
            MODULE.ALLOWED_EXCEPTIONS.update(original)

    def test_whitelist_entry_with_placeholder_reason_is_flagged(self) -> None:
        """白名单条目用 TODO 占位会报 no-reason-exception。"""
        original = dict(MODULE.ALLOWED_EXCEPTIONS)
        try:
            MODULE.ALLOWED_EXCEPTIONS[
                (Path("some/file.rs"), "god-file")
            ] = "TODO"
            findings = MODULE.scan_repository(Path("/nonexistent"))
            rules = {f.rule for f in findings}
            self.assertIn("no-reason-exception", rules)
        finally:
            MODULE.ALLOWED_EXCEPTIONS.clear()
            MODULE.ALLOWED_EXCEPTIONS.update(original)

    def test_whitelist_with_valid_reason_not_flagged(self) -> None:
        """白名单条目有具体原因不报 no-reason-exception。"""
        original = dict(MODULE.ALLOWED_EXCEPTIONS)
        try:
            MODULE.ALLOWED_EXCEPTIONS.clear()
            MODULE.ALLOWED_EXCEPTIONS[
                (Path("some/file.rs"), "god-file")
            ] = "该文件是数据模型聚合根，拆分会破坏契约一致性"
            findings = MODULE.scan_repository(Path("/nonexistent"))
            rules = {f.rule for f in findings}
            self.assertNotIn("no-reason-exception", rules)
        finally:
            MODULE.ALLOWED_EXCEPTIONS.clear()
            MODULE.ALLOWED_EXCEPTIONS.update(original)

    def test_god_file_whitelisted_file_not_flagged(self) -> None:
        """白名单中的 god-file 文件不报违规。"""
        original = dict(MODULE.ALLOWED_EXCEPTIONS)
        try:
            MODULE.ALLOWED_EXCEPTIONS.clear()
            MODULE.ALLOWED_EXCEPTIONS[
                (Path("whitelisted_big.rs"), "god-file")
            ] = "测试用白名单：验证豁免机制"
            lines = ["fn line_%d() {}" % i for i in range(801)]
            source = "\n".join(lines)
            self.assertNotIn("god-file", self.rule_names(source, path="whitelisted_big.rs"))
        finally:
            MODULE.ALLOWED_EXCEPTIONS.clear()
            MODULE.ALLOWED_EXCEPTIONS.update(original)

    # ------------------------------------------------------------------
    # 回归测试：Issue #597 移除的例外不得重新添加
    # ------------------------------------------------------------------

    def test_removed_kotlin_god_file_exceptions_not_readded(self) -> None:
        """Issue #597 要求拆分的 Kotlin 文件可重新加入 god-file 白名单，
        但原因必须包含具体技术理由和拆分/重构计划关键词。"""
        removed_paths = {
            Path("apps/android/app/src/main/kotlin/com/xiwei/sujian/feature/editor/EditorViewModel.kt"),
            Path("apps/android/app/src/main/kotlin/com/xiwei/sujian/feature/settings/ui/settings/SettingsRoute.kt"),
            Path("apps/android/app/src/main/kotlin/com/xiwei/sujian/feature/editor/coordinator/EditorSessionCoordinator.kt"),
        }
        required_keywords = ("拆分", "重构")
        for (path, rule), reason in MODULE.ALLOWED_EXCEPTIONS.items():
            if rule == "god-file" and path in removed_paths:
                self.assertTrue(
                    any(kw in reason for kw in required_keywords),
                    f"Issue #597 文件 {path} 重新加入白名单时，"
                    f"原因必须包含具体技术理由和拆分/重构计划关键词；"
                    f"当前原因: {reason}",
                )

    def test_whitelist_entry_reasons_are_substantive(self) -> None:
        """白名单原因必须说明'为什么不能拆分'而非'为什么大'。"""
        weak_patterns = ("行数略增", "行数增加", "重格式化")
        for (path, rule), reason in MODULE.ALLOWED_EXCEPTIONS.items():
            for pattern in weak_patterns:
                self.assertNotIn(
                    pattern,
                    reason,
                    f"白名单 {path}({rule}) 原因包含弱理由'{pattern}'；"
                    f"应说明为什么不能拆分而非为什么大",
                )

    # ------------------------------------------------------------------
    # 路径判断单元测试
    # ------------------------------------------------------------------

    def test_is_production_source_rust(self) -> None:
        self.assertTrue(MODULE.is_production_source(Path("src/main.rs")))
        self.assertFalse(MODULE.is_production_source(Path("src/tests.rs")))
        self.assertFalse(MODULE.is_production_source(Path("src/foo_tests.rs")))
        self.assertFalse(MODULE.is_production_source(Path("target/x.rs")))
        self.assertFalse(MODULE.is_production_source(Path("bindings/x.rs")))

    def test_is_production_source_kotlin(self) -> None:
        self.assertTrue(
            MODULE.is_production_source(
                Path("apps/android/app/src/main/kotlin/Foo.kt")
            )
        )
        self.assertFalse(
            MODULE.is_production_source(
                Path("apps/android/app/src/test/kotlin/Foo.kt")
            )
        )
        self.assertFalse(
            MODULE.is_production_source(
                Path("apps/android/app/src/main/kotlin/FooTest.kt")
            )
        )

    def test_is_production_source_kts(self) -> None:
        self.assertTrue(MODULE.is_production_source(Path("scripts/run.kts")))
        self.assertFalse(MODULE.is_production_source(Path("build.gradle.kts")))

    # ------------------------------------------------------------------
    # 有效行数计算单元测试
    # ------------------------------------------------------------------

    def test_count_effective_lines_basic(self) -> None:
        self.assertEqual(MODULE._count_effective_lines("fn a() {}\nfn b() {}"), 2)

    def test_count_effective_lines_skips_blanks(self) -> None:
        self.assertEqual(MODULE._count_effective_lines("\n\nfn a() {}\n\n"), 1)

    def test_count_effective_lines_skips_line_comments(self) -> None:
        source = "// comment\nfn a() {}\n// another\n"
        self.assertEqual(MODULE._count_effective_lines(source), 1)

    def test_count_effective_lines_skips_block_comments(self) -> None:
        source = "/* block\n multi line */\nfn a() {}\n"
        self.assertEqual(MODULE._count_effective_lines(source), 1)


if __name__ == "__main__":
    unittest.main()
