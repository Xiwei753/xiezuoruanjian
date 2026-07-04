#!/usr/bin/env python3
"""检查 QML 文件中的硬编码 hex 颜色。

扫描 apps/desktop/qml/ 目录下所有 .qml 文件（排除 DesignTokens.qml），
检测硬编码的 hex 颜色值（#[0-9a-fA-F]{3,8}），允许 transparent 关键字。

用法:
    python tools/check_qml_hex_colors.py

退出码:
    0 — 无硬编码颜色
    1 — 发现硬编码颜色
"""

import os
import re
import sys

QML_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                       "apps", "desktop", "qml")
EXCLUDED_FILES = {"DesignTokens.qml"}

# 匹配 # 后跟 3-8 个十六进制字符（3/4/6/8 位 hex 颜色）
HEX_COLOR_RE = re.compile(r"#[0-9a-fA-F]{3,8}\b")

# 允许的关键字
ALLOWED_KEYWORDS = {"transparent"}


def check_file(filepath: str, filename: str) -> list[tuple[int, str]]:
    """检查单个 QML 文件，返回 (行号, 颜色值) 列表。"""
    results = []
    with open(filepath, "r", encoding="utf-8") as f:
        for lineno, line in enumerate(f, start=1):
            # 跳过注释行
            stripped = line.lstrip()
            if stripped.startswith("//"):
                continue
            for match in HEX_COLOR_RE.finditer(line):
                color = match.group(0)
                # 排除 URL 中的 hash（如 #anchor）——长度 3-8 的纯 hex 才是颜色
                results.append((lineno, color))
    return results


def main() -> int:
    if not os.path.isdir(QML_DIR):
        print(f"QML 目录不存在: {QML_DIR}", file=sys.stderr)
        return 1

    violations: list[tuple[str, int, str]] = []  # (文件名, 行号, 颜色值)

    for filename in sorted(os.listdir(QML_DIR)):
        if not filename.endswith(".qml"):
            continue
        if filename in EXCLUDED_FILES:
            continue
        filepath = os.path.join(QML_DIR, filename)
        if not os.path.isfile(filepath):
            continue
        for lineno, color in check_file(filepath, filename):
            violations.append((filename, lineno, color))

    if violations:
        print("发现硬编码 hex 颜色:")
        for filename, lineno, color in violations:
            print(f"  {filename}:{lineno} {color}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
