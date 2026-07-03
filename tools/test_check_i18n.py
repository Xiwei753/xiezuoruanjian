#!/usr/bin/env python3
"""
test_check_i18n.py — check_i18n.py 核心函数单元测试

运行方式：
  python -m pytest tools/test_check_i18n.py -v
  python -m unittest tools.test_check_i18n -v
"""

import os
import sys
import tempfile
import unittest

# 将 tools 目录加入 path
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from check_i18n import (
    RE_QSTR,
    check_chinese_in_qstr,
    check_qstr_in_ts,
    collect_qstr_sources_from_qml,
    collect_sources_from_ts,
)


def _write_temp_qml(content: str) -> str:
    """将内容写入临时 QML 文件，返回文件路径"""
    fd, path = tempfile.mkstemp(suffix=".qml", prefix="test_i18n_")
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        f.write(content)
    return path


def _write_temp_ts(sources: list[str]) -> str:
    """创建一个简单的 ts XML 文件，包含给定的 source 列表"""
    fd, path = tempfile.mkstemp(suffix=".ts", prefix="test_i18n_")
    lines = [
        '<?xml version="1.0" encoding="utf-8"?>',
        '<!DOCTYPE TS>',
        '<TS version="2.1" language="zh_CN">',
        "<context>",
        "<name>TestContext</name>",
    ]
    for s in sources:
        lines.append("<message>")
        lines.append(f"<source>{s}</source>")
        lines.append('<translation type="unfinished"></translation>')
        lines.append("</message>")
    lines.append("</context>")
    lines.append("</TS>")
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
    return path


class TestChineseInQstr(unittest.TestCase):
    """check_chinese_in_qstr 函数测试"""

    def test_single_qstr(self):
        """单个 qsTr 包裹中文，不报错"""
        path = _write_temp_qml('Label { text: qsTr("你好世界") }\n')
        try:
            errs = check_chinese_in_qstr([path])
            self.assertEqual(errs, [])
        finally:
            os.unlink(path)

    def test_unwrapped_chinese(self):
        """中文不在 qsTr 中，报错"""
        path = _write_temp_qml('Label { text: "你好世界" }\n')
        try:
            errs = check_chinese_in_qstr([path])
            self.assertTrue(len(errs) > 0, "应检测到未包裹的中文")
        finally:
            os.unlink(path)

    def test_multiple_qstr_same_line(self):
        """同一行多个 qsTr，都正确包裹不报错"""
        path = _write_temp_qml(
            'Label { text: qsTr("你好") + qsTr("世界") }\n'
        )
        try:
            errs = check_chinese_in_qstr([path])
            self.assertEqual(errs, [])
        finally:
            os.unlink(path)

    def test_partial_wrap_same_line(self):
        """同一行一个 qsTr 包裹了中文，另一个中文未包裹，报错"""
        path = _write_temp_qml(
            'Label { text: qsTr("你好") + "世界" }\n'
        )
        try:
            errs = check_chinese_in_qstr([path])
            self.assertTrue(len(errs) > 0, "应检测到部分未包裹的中文")
        finally:
            os.unlink(path)

    def test_escaped_quotes(self):
        """qsTr 内含转义引号 qsTr("他说\"你好\"")，正确提取"""
        path = _write_temp_qml('Label { text: qsTr("他说\\"你好\\"") }\n')
        try:
            errs = check_chinese_in_qstr([path])
            self.assertEqual(errs, [], "转义引号内的中文应被正确识别为已包裹")
        finally:
            os.unlink(path)

    def test_single_quotes(self):
        """qsTr 使用单引号 qsTr('中文文本')，正确提取"""
        path = _write_temp_qml("Label { text: qsTr('你好世界') }\n")
        try:
            errs = check_chinese_in_qstr([path])
            self.assertEqual(errs, [], "单引号 qsTr 中的中文应被正确识别为已包裹")
        finally:
            os.unlink(path)

    def test_multiline_string(self):
        """跨行字符串拼接，检查不会误杀"""
        # QML 中字符串拼接不在同一行，第一行有 qsTr 开头
        path = _write_temp_qml(
            'Label { text: qsTr("你好") +\n"世界" }\n'
        )
        try:
            errs = check_chinese_in_qstr([path])
            # 第二行 "世界" 不在 qsTr 中，可能被检测为未包裹
            # 这是预期行为：跨行拼接时第二行的中文确实未被 qsTr 包裹
            # 测试确保不会崩溃即可
        finally:
            os.unlink(path)

    def test_comment_line(self):
        """注释行中的中文不报错"""
        path = _write_temp_qml('// 这是一个注释，包含中文\n')
        try:
            errs = check_chinese_in_qstr([path])
            self.assertEqual(errs, [], "注释行中的中文不应报错")
        finally:
            os.unlink(path)

    def test_debug_line(self):
        """console.log/debugLog 行中的中文不报错"""
        path = _write_temp_qml('console.log("调试信息")\n')
        try:
            errs = check_chinese_in_qstr([path])
            self.assertEqual(errs, [], "console.log 中的中文不应报错")
        finally:
            os.unlink(path)


class TestRE_QSTR(unittest.TestCase):
    """RE_QSTR 正则表达式测试"""

    def test_qstr_regex_escaped_quote(self):
        """RE_QSTR 正则能正确匹配含转义引号的 qsTr"""
        # 注意：Python 字符串中 \" 需要转义为 \\\"
        m = RE_QSTR.search('qsTr("他说\\"你好\\"")')
        self.assertIsNotNone(m, "应匹配含转义引号的 qsTr")

    def test_qstr_regex_single_quote(self):
        """RE_QSTR 正则能正确匹配单引号 qsTr"""
        m = RE_QSTR.search("qsTr('中文文本')")
        self.assertIsNotNone(m, "应匹配单引号 qsTr")
        self.assertEqual(m.group(1), "中文文本")


class TestTsSourceCollection(unittest.TestCase):
    """collect_sources_from_ts 函数测试"""

    def test_ts_source_collection(self):
        """collect_sources_from_ts 从 ts 文件提取 source"""
        path = _write_temp_ts(["你好世界", "保存", "取消"])
        try:
            sources = collect_sources_from_ts(path)
            self.assertEqual(sources, {"你好世界", "保存", "取消"})
        finally:
            os.unlink(path)


class TestQstrInTs(unittest.TestCase):
    """check_qstr_in_ts 函数测试"""

    def test_qstr_in_ts_missing(self):
        """qsTr 中有但 ts 中没有的文本被检测到"""
        qml_sources = {"你好", "世界", "保存"}
        ts_sources = {"你好", "保存"}
        missing = check_qstr_in_ts(qml_sources, ts_sources)
        self.assertEqual(missing, ["世界"])

    def test_qstr_in_ts_present(self):
        """qsTr 和 ts 都有的文本不报错"""
        qml_sources = {"你好", "世界"}
        ts_sources = {"你好", "世界", "其他"}
        missing = check_qstr_in_ts(qml_sources, ts_sources)
        self.assertEqual(missing, [])


class TestCollectQstrSourcesFromQml(unittest.TestCase):
    """collect_qstr_sources_from_qml 函数测试"""

    def test_collect_chinese_qstr(self):
        """从 QML 文件中提取含中文的 qsTr 文本"""
        path = _write_temp_qml(
            'Label { text: qsTr("你好") }\nLabel { text: qsTr("hello") }\n'
        )
        try:
            sources = collect_qstr_sources_from_qml([path])
            self.assertIn("你好", sources)
            self.assertNotIn("hello", sources)  # 不含中文，不应被收集
        finally:
            os.unlink(path)


if __name__ == "__main__":
    unittest.main()
