#!/usr/bin/env python3
"""Android Kotlin 中文硬编码检查脚本。

扫描 Android Kotlin 源码，检测字符串字面量中的中文字符（Unicode \u4e00-\u9fff），
防止新增中文硬编码 UI 文案。注释、日志（Log/warn）、测试文件、行级豁免标记中的中文不报错。

行级豁免：在 Kotlin 代码行末添加 `// i18n-exempt` 注释，该行的中文字符串不报错。
自动豁免：注释行、Log.d/e/w/i/v 调用行、warn() 调用行自动跳过，无需标记。

同时检查 strings.xml 格式有效性。

用法:
    python3 tools/check_android_i18n.py [--verbose]

返回码:
    0 = 通过
    1 = 有错误
"""

import argparse
import os
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

# ── 配置 ──────────────────────────────────────────────────────────────────────

# 项目根目录（脚本所在目录的上一级）
PROJECT_ROOT = Path(__file__).resolve().parent.parent

# Kotlin 源码目录
KOTLIN_SRC_DIR = PROJECT_ROOT / "apps" / "android" / "app" / "src" / "main" / "kotlin"

# strings.xml 路径
STRINGS_XML_PATH = (
    PROJECT_ROOT / "apps" / "android" / "app" / "src" / "main" / "res" / "values" / "strings.xml"
)

# 中文字符 Unicode 范围
CHINESE_PATTERN = re.compile(r"[\u4e00-\u9fff]")

# Kotlin 字符串字面量正则（双引号字符串，支持转义引号 \"）
# 匹配 "..." 内的内容，处理 \" 转义
STRING_LITERAL_RE = re.compile(r'"((?:[^"\\]|\\.)*)"', re.DOTALL)

# Log 调用正则（Log.d / Log.e / Log.w / Log.i / Log.v）
LOG_CALL_RE = re.compile(r"\bLog\.[dewiv]\s*\(")

# warn() 调用正则（DiagnosticsLogger.warn，与 Log 同理属于诊断日志）
WARN_CALL_RE = re.compile(r"\bwarn\s*\(")

# 注释行正则（行首空白后以 // 或 /* 或 * 开头）
COMMENT_LINE_RE = re.compile(r"^\s*(//|/\*|\*)")

# 行尾注释正则（匹配 // 及其后的所有内容，但不在字符串字面量内）
TRAILING_COMMENT_RE = re.compile(r'(?<!\\)//.*$')

# 行级豁免标记正则（行末 // i18n-exempt 表示该行中文字符串允许豁免）
I18N_EXEMPT_RE = re.compile(r"//\s*i18n-exempt\s*$")

# 文件名包含 Test/test 的视为测试文件，跳过检查
TEST_FILE_RE = re.compile(r"[Tt]est")


# ── Kotlin 中文硬编码检查 ────────────────────────────────────────────────────

def _strip_trailing_comment(line: str) -> str:
    """移除行尾 // 注释，但保留字符串字面量内的 //。

    简单策略：逐字符扫描，跟踪引号状态，只移除引号外的 //。
    """
    in_string = False
    i = 0
    while i < len(line):
        ch = line[i]
        if ch == '\\' and in_string:
            # 跳过转义字符
            i += 2
            continue
        if ch == '"':
            in_string = not in_string
        if not in_string and i + 1 < len(line) and line[i:i + 2] == '//':
            return line[:i]
        i += 1
    return line


def check_kotlin_file(filepath: Path, verbose: bool = False) -> list[tuple[int, str, str]]:
    """检查单个 Kotlin 文件中字符串字面量是否包含中文。

    Args:
        filepath: Kotlin 文件路径
        verbose: 是否输出详细信息

    Returns:
        错误列表，每项为 (行号, 匹配的字符串, 所在行内容)
    """
    errors = []

    # 测试文件跳过
    if TEST_FILE_RE.search(filepath.name):
        if verbose:
            print(f"  SKIP (test file): {filepath.name}")
        return errors

    try:
        content = filepath.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as e:
        errors.append((0, "", f"无法读取文件: {e}"))
        return errors

    lines = content.split("\n")

    for line_num, line in enumerate(lines, start=1):
        stripped = line.strip()

        # 跳过纯注释行
        if COMMENT_LINE_RE.match(stripped):
            continue

        # 跳过 Log 调用行（日志允许中文）
        if LOG_CALL_RE.search(line):
            continue

        # 跳过 warn() 调用行（DiagnosticsLogger.warn 诊断日志，允许中文）
        if WARN_CALL_RE.search(line):
            continue

        # 跳过行级豁免标记（行末 // i18n-exempt）
        if I18N_EXEMPT_RE.search(line):
            continue

        # 移除行尾注释后再检查字符串字面量
        code_part = _strip_trailing_comment(line)

        # 查找所有字符串字面量
        for match in STRING_LITERAL_RE.finditer(code_part):
            string_content = match.group(1)
            if CHINESE_PATTERN.search(string_content):
                # 显示原始字符串（含引号）
                full_match = f'"{string_content}"'
                errors.append((line_num, full_match, stripped))

    return errors


def check_all_kotlin_files(verbose: bool = False) -> int:
    """扫描所有 Kotlin 文件，检查中文硬编码。

    Returns:
        错误总数
    """
    if not KOTLIN_SRC_DIR.exists():
        print(f"FAIL: Kotlin 源码目录不存在: {KOTLIN_SRC_DIR}")
        return -1

    kt_files = sorted(KOTLIN_SRC_DIR.rglob("*.kt"))
    if not kt_files:
        print("PASS: 未找到 Kotlin 文件（目录为空）")
        return 0

    if verbose:
        print(f"扫描 {len(kt_files)} 个 Kotlin 文件...")

    total_errors = 0
    error_files = 0

    for kt_file in kt_files:
        rel_path = kt_file.relative_to(PROJECT_ROOT)
        errors = check_kotlin_file(kt_file, verbose)
        if errors:
            error_files += 1
            for line_num, matched_str, line_content in errors:
                total_errors += 1
                print(
                    f"  {rel_path}:{line_num}: "
                    f"中文字符串字面量 {matched_str}"
                )

    if total_errors > 0:
        print(f"\nFAIL: 发现 {total_errors} 处 Kotlin 中文字符串硬编码 "
              f"（涉及 {error_files} 个文件）")
        print("提示: UI 文案应放入 strings.xml，注释/日志/warn()/i18n-exempt 除外")
    else:
        print(f"PASS: {len(kt_files)} 个 Kotlin 文件无中文字符串硬编码")

    return total_errors


# ── strings.xml 格式检查 ─────────────────────────────────────────────────────

def check_strings_xml(verbose: bool = False) -> int:
    """检查 strings.xml 格式有效性。

    Returns:
        0 = 通过，1 = 有错误
    """
    if not STRINGS_XML_PATH.exists():
        print(f"FAIL: strings.xml 不存在: {STRINGS_XML_PATH}")
        return 1

    try:
        tree = ET.parse(STRINGS_XML_PATH)
        root = tree.getroot()
    except ET.ParseError as e:
        print(f"FAIL: strings.xml 解析失败: {e}")
        return 1

    if root.tag != "resources":
        print(f"FAIL: strings.xml 根元素不是 <resources>，而是 <{root.tag}>")
        return 1

    string_count = 0
    errors = 0

    for elem in root:
        if elem.tag == "string":
            string_count += 1
            name = elem.get("name")
            if not name:
                errors += 1
                print(f"  FAIL: strings.xml 第 {elem.sourceline} 行: "
                      f"<string> 缺少 name 属性")
            # 检查是否有空值（可能遗漏翻译）
            text = (elem.text or "").strip()
            if not text:
                if verbose:
                    print(f"  WARN: strings.xml: <string name=\"{name}\"> 值为空")

    if errors > 0:
        print(f"FAIL: strings.xml 有 {errors} 处格式错误")
        return 1
    else:
        print(f"PASS: strings.xml 格式有效（{string_count} 个字符串条目）")
        return 0


# ── 主入口 ────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(
        description="检查 Android Kotlin 源码中文字符串硬编码"
    )
    parser.add_argument(
        "--verbose", "-v",
        action="store_true",
        help="输出详细信息（包括跳过的文件）"
    )
    args = parser.parse_args()

    print("=" * 60)
    print("Android i18n 检查：Kotlin 中文字符串硬编码")
    print("=" * 60)

    # 1. Kotlin 中文硬编码检查
    print("\n[1/2] Kotlin 源码中文字符串检查")
    print("-" * 40)
    kotlin_errors = check_all_kotlin_files(verbose=args.verbose)

    # 2. strings.xml 格式检查
    print("\n[2/2] strings.xml 格式有效性检查")
    print("-" * 40)
    xml_result = check_strings_xml(verbose=args.verbose)

    # 汇总结果
    print("\n" + "=" * 60)
    if kotlin_errors == 0 and xml_result == 0:
        print("全部通过")
        sys.exit(0)
    else:
        if kotlin_errors < 0:
            # 目录不存在等致命错误
            sys.exit(1)
        print(f"未通过（Kotlin 中文硬编码: {kotlin_errors} 处, "
              f"strings.xml 错误: {xml_result} 处）")
        sys.exit(1)


if __name__ == "__main__":
    main()
