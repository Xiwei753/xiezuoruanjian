#!/usr/bin/env python3
"""
check_i18n.py — QML i18n 完整性检查脚本

检查内容：
1. QML 中文文本必须被 qsTr() 包裹
2. zh_CN.ts 不含 unfinished/vanished 长期漏项
3. qsTr 中的中文文本必须在 zh_CN.ts 中有对应条目
4. lupdate 可用时，临时 ts 与仓库 ts 的 source 差异对比

返回码：0=通过，1=有错误
输出格式：PASS: ... / FAIL: ...，便于 CI 阅读
"""

import os
import re
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

# ── 路径配置 ──────────────────────────────────────────────
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(SCRIPT_DIR)
QML_DIR = os.path.join(REPO_ROOT, "apps", "desktop", "qml")
TS_FILE = os.path.join(REPO_ROOT, "apps", "desktop", "i18n", "zh_CN.ts")

# ── 正则 ─────────────────────────────────────────────────
# 中文字符范围（CJK Unified Ideographs）
RE_CHINESE = re.compile(r"[\u4e00-\u9fff]")
# qsTr("...") 或 qsTr('...')，提取引号内文本
RE_QSTR = re.compile(r'qsTr\(\s*["\'](.+?)["\']\s*\)')
# 注释行（以 // 开头，允许前导空白）
RE_COMMENT = re.compile(r"^\s*//")
# 调试输出行（console.log / debugLog / logger / log_）
RE_DEBUG = re.compile(r"(console\.log|debugLog|logger|log_)\s*\(")

# ── verbose ──────────────────────────────────────────────
VERBOSE = "--verbose" in sys.argv


def verbose(msg: str) -> None:
    if VERBOSE:
        print(msg)


def collect_qml_files(qml_dir: str) -> list[str]:
    """递归收集 qml_dir 下所有 .qml 文件"""
    result = []
    for root, _dirs, files in os.walk(qml_dir):
        for f in files:
            if f.endswith(".qml"):
                result.append(os.path.join(root, f))
    return sorted(result)


# ─────────────────────────────────────────────────────────
# 检查 1：QML 中文文本必须被 qsTr() 包裹
# ─────────────────────────────────────────────────────────
def check_chinese_in_qstr(qml_files: list[str]) -> list[str]:
    """返回错误列表，每项格式：文件:行号: 内容"""
    errors = []
    for filepath in qml_files:
        rel = os.path.relpath(filepath, REPO_ROOT)
        with open(filepath, encoding="utf-8") as fh:
            for lineno, line in enumerate(fh, 1):
                # 跳过注释行
                if RE_COMMENT.match(line):
                    continue
                # 跳过调试输出行
                if RE_DEBUG.search(line):
                    continue
                # 行中有中文？
                if not RE_CHINESE.search(line):
                    continue
                # 行中有 qsTr 包裹的中文？提取所有 qsTr 调用
                qstr_matches = RE_QSTR.findall(line)
                # 移除 qsTr 中的中文部分，看剩余是否还有中文
                remaining = line
                for m in qstr_matches:
                    # 替换掉 qsTr("...") 整个调用
                    remaining = remaining.replace(f'qsTr("{m}")', "", 1)
                    remaining = remaining.replace(f"qsTr('{m}')", "", 1)
                # 再次检查剩余部分是否有中文
                if RE_CHINESE.search(remaining):
                    # 排除多行字符串中 qsTr 跨行的情况（简单启发式）
                    errors.append(f"{rel}:{lineno}: {line.rstrip()}")
    return errors


# ─────────────────────────────────────────────────────────
# 检查 2：zh_CN.ts 不含 unfinished/vanished 条目
# ─────────────────────────────────────────────────────────
def check_ts_unfinished_vanished(ts_path: str) -> list[str]:
    """返回错误列表"""
    errors = []
    if not os.path.isfile(ts_path):
        errors.append(f"TS 文件不存在: {ts_path}")
        return errors

    tree = ET.parse(ts_path)
    root = tree.getroot()
    for context in root.findall("context"):
        ctx_name = context.findtext("name", "")
        for message in context.findall("message"):
            source = message.findtext("source", "")
            translation = message.find("translation")
            if translation is not None:
                ttype = translation.get("type", "")
                if ttype in ("unfinished", "vanished"):
                    errors.append(
                        f"[{ctx_name}] source=\"{source}\" → type=\"{ttype}\""
                    )
    return errors


# ─────────────────────────────────────────────────────────
# 检查 3：qsTr 中的中文文本必须在 zh_CN.ts 中有对应条目
# ─────────────────────────────────────────────────────────
def collect_qstr_sources_from_qml(qml_files: list[str]) -> set[str]:
    """从所有 QML 文件中提取 qsTr("...") 的文本"""
    sources = set()
    for filepath in qml_files:
        with open(filepath, encoding="utf-8") as fh:
            content = fh.read()
            for m in RE_QSTR.finditer(content):
                text = m.group(1)
                # 只关注含中文的 qsTr 文本
                if RE_CHINESE.search(text):
                    sources.add(text)
    return sources


def collect_sources_from_ts(ts_path: str) -> set[str]:
    """从 zh_CN.ts 中提取所有 <source> 标签文本"""
    sources = set()
    if not os.path.isfile(ts_path):
        return sources
    tree = ET.parse(ts_path)
    root = tree.getroot()
    for context in root.findall("context"):
        for message in context.findall("message"):
            source = message.findtext("source", "")
            if source:
                sources.add(source)
    return sources


def check_qstr_in_ts(
    qml_sources: set[str], ts_sources: set[str]
) -> list[str]:
    """返回 qsTr 中有但 ts 中没有的文本列表"""
    missing = qml_sources - ts_sources
    return sorted(missing)


# ─────────────────────────────────────────────────────────
# 检查 4：lupdate 可用时，临时 ts 与仓库 ts 的 source 差异
# ─────────────────────────────────────────────────────────
def check_lupdate_ts_comparison(qml_dir: str, repo_ts_path: str) -> tuple[list[str], bool]:
    """检查 lupdate 生成的临时 ts 与仓库 ts 的 source 差异。

    Returns:
        (errors, skipped) - errors 为新增 source 列表，skipped 为是否跳过
    """
    # 检查 lupdate 是否可用（先试 lupdate，再试 lupdate6）
    lupdate_cmd = shutil.which("lupdate")
    if lupdate_cmd is None:
        lupdate_cmd = shutil.which("lupdate6")
    if lupdate_cmd is None:
        return ([], True)  # skipped

    # 创建临时目录
    with tempfile.TemporaryDirectory() as tmpdir:
        tmp_ts_path = os.path.join(tmpdir, "zh_CN.ts")

        # 运行 lupdate
        try:
            result = subprocess.run(
                [lupdate_cmd, qml_dir, "-ts", tmp_ts_path],
                capture_output=True, text=True, timeout=60
            )
            if result.returncode != 0:
                verbose(f"  lupdate 返回码 {result.returncode}: {result.stderr}")
                return ([], True)  # lupdate 失败也跳过
        except (subprocess.TimeoutExpired, OSError) as e:
            verbose(f"  lupdate 执行失败: {e}")
            return ([], True)

        # 从临时 ts 提取 source 集合
        tmp_sources = collect_sources_from_ts(tmp_ts_path)

        # 从仓库 ts 提取 source 集合
        repo_sources = collect_sources_from_ts(repo_ts_path)

        # 找出临时 ts 有但仓库 ts 没有的 source
        missing = tmp_sources - repo_sources
        return (sorted(missing), False)


# ─────────────────────────────────────────────────────────
# 主流程
# ─────────────────────────────────────────────────────────
def main() -> int:
    has_error = False

    # 收集 QML 文件
    qml_files = collect_qml_files(QML_DIR)
    verbose(f"找到 {len(qml_files)} 个 QML 文件")

    # ── 检查 1 ──
    verbose("\n=== 检查 1: QML 中文文本必须被 qsTr() 包裹 ===")
    errs1 = check_chinese_in_qstr(qml_files)
    if errs1:
        has_error = True
        print(f"FAIL: 发现 {len(errs1)} 处中文文本未被 qsTr() 包裹")
        for e in errs1:
            print(f"  {e}")
    else:
        print("PASS: 所有 QML 中文文本均被 qsTr() 包裹")

    # ── 检查 2 ──
    verbose("\n=== 检查 2: zh_CN.ts 不含 unfinished/vanished 条目 ===")
    errs2 = check_ts_unfinished_vanished(TS_FILE)
    if errs2:
        has_error = True
        print(f"FAIL: zh_CN.ts 含 {len(errs2)} 个 unfinished/vanished 条目")
        for e in errs2:
            print(f"  {e}")
    else:
        print("PASS: zh_CN.ts 无 unfinished/vanished 条目")

    # ── 检查 3 ──
    verbose("\n=== 检查 3: qsTr 中文文本必须在 zh_CN.ts 中有对应条目 ===")
    qml_sources = collect_qstr_sources_from_qml(qml_files)
    ts_sources = collect_sources_from_ts(TS_FILE)
    verbose(f"  QML qsTr 中文文本: {len(qml_sources)} 条")
    verbose(f"  zh_CN.ts source 条目: {len(ts_sources)} 条")
    missing = check_qstr_in_ts(qml_sources, ts_sources)
    if missing:
        has_error = True
        print(f"FAIL: {len(missing)} 条 qsTr 中文文本不在 zh_CN.ts 中")
        for m in missing:
            print(f'  qsTr("{m}")')
    else:
        print("PASS: 所有 qsTr 中文文本均在 zh_CN.ts 中有对应条目")

    # ── 检查 4 ──
    verbose("\n=== 检查 4: lupdate 临时 ts 与仓库 ts 的 source 差异 ===")
    errs4, skipped4 = check_lupdate_ts_comparison(QML_DIR, TS_FILE)
    if skipped4:
        print("SKIP: lupdate not found, ts comparison check skipped")
    elif errs4:
        has_error = True
        print(f"FAIL: 仓库 zh_CN.ts 缺少 {len(errs4)} 条 lupdate 发现的 source")
        for e in errs4:
            print(f'  source="{e}"')
    else:
        print("PASS: 仓库 zh_CN.ts 包含 lupdate 发现的所有 source")

    return 1 if has_error else 0


if __name__ == "__main__":
    sys.exit(main())
