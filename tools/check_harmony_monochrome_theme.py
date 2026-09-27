#!/usr/bin/env python3
"""HarmonyOS 单色强调色 / 深色模式图标守卫（Issue #784）。

Issue #784 的三条原始要求里，有两条只能靠"不许再出现第二份真相"来维持：

1. 蓝色强调色来自系统默认主题和遗留的蓝色资源表条目，不是图片问题。
   应用级强调色只能由 `HarmonyMonochromeTheme.ets` 的
   `ThemeControl.setDefaultTheme()` 下发（浅色黑 / 深色白），
   页面和资源表都不能再各自声明强调色。
2. 深色模式下右上角图标不反色，来自自定义 SVG 里写死的 `fill` 和
   `Image` 缺少 `.fillColor()`。这两处只要回退一处，实机就又变成黑图标压在深色背景上。
   （评论 5858361456 / 5858404437 / 5858437476 连续三轮卡在这里，
     所以必须用可执行检查固定，而不是靠提交说明。）

检查项：

  1. svg-tintable          `apps/harmony/.../base/media/*.svg` 的 fill 只能是
                           currentColor/none，不能写死 #hex / rgb() / 颜色名。
  2. image-fill-color      ets 里渲染这些可着色 SVG（或 WritingScreen 页头动作图标
                           `getHeaderActionIcon(...)`）的 `Image` 必须带 `.fillColor(...)`。
  3. accent-color-table    `base/element/color.json` 不再声明 primary/secondary/accent
                           颜色（module.json5 依赖的 start_window_background 必须保留）。
  4. no-blue-accent-source ets 里不再出现 Color.Blue / sys.color.brand / sys.color.activated
                           以及已退休的 #4A90D9 / #7B68EE 蓝色强调色。
  5. theme-single-source   Color.Black/White 只能出现在 HarmonyMonochromeTheme.ets，
                           页面不得给单个控件补 `.selectedColor(...)`，
                           且 HarmonyThemeAdapter 必须在 effectiveColorMode 变化时
                           调用 `applySujianMonochromeTheme(...)`。

返回：0=通过，1=有违规。
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

HARMONY_ETS_REL = "apps/harmony/entry/src/main/ets"
HARMONY_MEDIA_REL = "apps/harmony/entry/src/main/resources/base/media"
HARMONY_COLOR_TABLE_REL = "apps/harmony/entry/src/main/resources/base/element/color.json"
MONOCHROME_THEME_REL = "apps/harmony/entry/src/main/ets/ui/theme/HarmonyMonochromeTheme.ets"
THEME_ADAPTER_REL = "apps/harmony/entry/src/main/ets/ui/theme/HarmonyThemeAdapter.ets"

# 应用级主题的唯一入口函数名（HarmonyMonochromeTheme.ets 导出，HarmonyThemeAdapter 调用）。
MONOCHROME_ENTRY = "applySujianMonochromeTheme"
# start_window_background 被 module.json5 的 startWindowBackground 引用，不能删。
START_WINDOW_COLOR_NAME = "start_window_background"

# SVG 里允许的 fill：currentColor 才能被 Image.fillColor 覆盖，none 是透明。
ALLOWED_FILL_VALUES = {"currentcolor", "none"}

FILL_DECL_RE = re.compile(r"""fill\s*[:=]\s*['"]?([^'";\s>]+)""", re.IGNORECASE)
IMAGE_CALL_RE = re.compile(r"(?<![\w.])Image\s*\(")
APP_ICON_REF_RE = re.compile(r"""\$r\(\s*['"]app\.media\.([A-Za-z0-9_]+)['"]\s*\)""")
FILL_COLOR_RE = re.compile(r"\.fillColor\s*\(")
HEADER_ICON_HELPER_RE = re.compile(r"getHeaderActionIcon\s*\(")
# 强调色资源名只在首段判定：primary_color / secondary_color / accent_bg 命中，
# text_primary / divider_color 这类中性语义名不命中。
ACCENT_RESOURCE_NAME_RE = re.compile(r"^(?:primary|secondary|tertiary|accent|brand)(?:_|$)", re.IGNORECASE)

# 已退休的蓝色强调色字面量只在主题层判定：星图节点 kind 的分类色板
# （Character='#4A90D9' 等）是图表标记色，不是应用强调色。
THEME_DIR_PREFIX = "apps/harmony/entry/src/main/ets/ui/theme/"
RETIRED_BLUE_LITERALS = ("#4a90d9", "#7b68ee", "#0000ff")

# 评论 5856805657：强调色统一由应用级主题下发，页面不得单独声明。
BLUE_ACCENT_SOURCE_PATTERNS = (
    (re.compile(r"\bColor\.Blue\b"), "Color.Blue"),
    (re.compile(r"\bsys\.color\.brand\b"), "sys.color.brand"),
    (re.compile(r"\bsys\.color\.activated\b"), "sys.color.activated"),
)
HARD_EMPHASIS_RE = re.compile(r"\bColor\.(?:Black|White)\b")
WIDGET_ACCENT_OVERRIDE_RE = re.compile(r"\.selectedColor\s*\(")


@dataclass(frozen=True)
class Finding:
    rule: str
    where: str
    message: str


@dataclass(frozen=True)
class ImageStatement:
    """一条 ArkUI `Image(...)` 语句（含后续 `.modifier()` 链）。"""

    line: int
    end_line: int
    text: str


def strip_comments(text: str) -> str:
    """逐行去掉 // 注释，保持行号不变（字符串里的 // 不动）。"""
    cleaned: list[str] = []
    for raw in text.splitlines():
        in_single = False
        in_double = False
        cut: int | None = None
        index = 0
        while index < len(raw):
            char = raw[index]
            if char == "'" and not in_double:
                in_single = not in_single
            elif char == '"' and not in_single:
                in_double = not in_double
            elif (
                char == "/"
                and not in_single
                and not in_double
                and index + 1 < len(raw)
                and raw[index + 1] == "/"
            ):
                cut = index
                break
            index += 1
        cleaned.append(raw if cut is None else raw[:cut])
    return "\n".join(cleaned)


def _paren_balance(text: str) -> int:
    return text.count("(") - text.count(")")


def iter_image_statements(clean_text: str) -> list[ImageStatement]:
    """切出 `Image(...)` 语句：跨行参数补齐，然后吃掉以 '.' 开头的 modifier 链。"""
    lines = clean_text.splitlines()
    statements: list[ImageStatement] = []
    index = 0
    while index < len(lines):
        if IMAGE_CALL_RE.search(lines[index]) is None:
            index += 1
            continue
        start = index
        chunk = lines[index]
        while _paren_balance(chunk) > 0 and index + 1 < len(lines):
            index += 1
            chunk += "\n" + lines[index]
        end = index
        index += 1
        while index < len(lines):
            stripped = lines[index].strip()
            if stripped == "":
                index += 1
                continue
            if not stripped.startswith("."):
                break
            chunk += "\n" + stripped
            end = index
            index += 1
        statements.append(ImageStatement(line=start + 1, end_line=end + 1, text=chunk))
    return statements


def check_svg(name: str, text: str) -> list[Finding]:
    """规则 1：SVG 必须可着色。"""
    findings: list[Finding] = []
    for match in FILL_DECL_RE.finditer(text):
        value = match.group(1).strip().strip("'\"")
        if value.lower() in ALLOWED_FILL_VALUES:
            continue
        findings.append(
            Finding(
                "svg-tintable",
                f"{HARMONY_MEDIA_REL}/{name}",
                f"fill={value!r} 是固定颜色，必须改成 currentColor，"
                "否则 Image.fillColor 覆盖不了、深色模式不会反色",
            )
        )
    return findings


def check_ets_source(path_rel: str, text: str, tintable_icons: set[str]) -> list[Finding]:
    """规则 2：渲染可着色 SVG / 页头动作图标的 Image 必须带 fillColor。"""
    findings: list[Finding] = []
    for statement in iter_image_statements(strip_comments(text)):
        icon_match = APP_ICON_REF_RE.search(statement.text)
        reason = ""
        if icon_match is not None and icon_match.group(1) in tintable_icons:
            reason = f"渲染可着色 SVG app.media.{icon_match.group(1)}"
        elif HEADER_ICON_HELPER_RE.search(statement.text):
            reason = "渲染页头动作图标 getHeaderActionIcon(...)"
        if reason == "" or FILL_COLOR_RE.search(statement.text):
            continue
        findings.append(
            Finding(
                "image-fill-color",
                f"{path_rel}:{statement.line}",
                f"{reason} 的 Image 缺少 .fillColor($r('sys.color.icon_primary'))，"
                "深色模式下会保持固定黑色",
            )
        )
    return findings


def check_color_table(table: object) -> list[Finding]:
    """规则 3：资源表不再声明强调色，且保留 module.json5 依赖的启动窗口色。"""
    if not isinstance(table, dict) or not isinstance(table.get("color"), list):
        return [
            Finding(
                "accent-color-table",
                HARMONY_COLOR_TABLE_REL,
                '顶层必须是 {"color": [...]}',
            )
        ]
    findings: list[Finding] = []
    names: list[str] = []
    for index, entry in enumerate(table["color"]):
        if not isinstance(entry, dict) or not isinstance(entry.get("name"), str):
            findings.append(
                Finding("accent-color-table", f"{HARMONY_COLOR_TABLE_REL}[{index}]", "条目缺少字符串 name")
            )
            continue
        name = entry["name"]
        names.append(name)
        if ACCENT_RESOURCE_NAME_RE.search(name):
            findings.append(
                Finding(
                    "accent-color-table",
                    f"{HARMONY_COLOR_TABLE_REL}:{name}",
                    "强调色只能由应用级主题（HarmonyMonochromeTheme）下发，"
                    "资源表里不能再有 primary/secondary/accent 颜色",
                )
            )
    if START_WINDOW_COLOR_NAME not in names:
        findings.append(
            Finding(
                "accent-color-table",
                HARMONY_COLOR_TABLE_REL,
                f"module.json5 的 startWindowBackground 依赖 $color:{START_WINDOW_COLOR_NAME}，不能删除",
            )
        )
    return findings


def check_blue_accent_source(path_rel: str, text: str) -> list[Finding]:
    """规则 4：ets 里不再出现蓝色强调色。"""
    clean = strip_comments(text)
    findings: list[Finding] = []
    for pattern, label in BLUE_ACCENT_SOURCE_PATTERNS:
        if pattern.search(clean) is not None:
            findings.append(
                Finding(
                    "no-blue-accent-source",
                    path_rel,
                    f"出现 {label}：强调色必须来自应用级单色主题（浅色黑 / 深色白）",
                )
            )
    if path_rel.startswith(THEME_DIR_PREFIX):
        lowered = clean.lower()
        for literal in RETIRED_BLUE_LITERALS:
            if literal in lowered:
                findings.append(
                    Finding(
                        "no-blue-accent-source",
                        path_rel,
                        f"主题层出现已退休的蓝色强调色 {literal}：应用级主题才是唯一入口",
                    )
                )
    return findings


def check_theme_single_source(path_rel: str, text: str) -> list[Finding]:
    """规则 5：单色强调色只有一个来源。"""
    clean = strip_comments(text)
    findings: list[Finding] = []
    if path_rel != MONOCHROME_THEME_REL and HARD_EMPHASIS_RE.search(clean) is not None:
        findings.append(
            Finding(
                "theme-single-source",
                path_rel,
                "Color.Black/Color.White 只能出现在 HarmonyMonochromeTheme.ets，"
                "页面不要给单个控件补黑白色",
            )
        )
    if path_rel != MONOCHROME_THEME_REL and WIDGET_ACCENT_OVERRIDE_RE.search(clean) is not None:
        findings.append(
            Finding(
                "theme-single-source",
                path_rel,
                "页面不得用 .selectedColor(...) 给单个控件补强调色，"
                "Toggle/Slider 选中色由应用级主题统一提供",
            )
        )
    return findings


def check_repository(root: Path) -> list[Finding]:
    findings: list[Finding] = []

    media_dir = root / HARMONY_MEDIA_REL
    tintable_icons: set[str] = set()
    if not media_dir.is_dir():
        findings.append(Finding("svg-tintable", HARMONY_MEDIA_REL, "媒体资源目录不存在"))
    else:
        for svg in sorted(media_dir.glob("*.svg")):
            tintable_icons.add(svg.stem)
            findings.extend(check_svg(svg.name, svg.read_text(encoding="utf-8")))

    color_path = root / HARMONY_COLOR_TABLE_REL
    if not color_path.is_file():
        findings.append(Finding("accent-color-table", HARMONY_COLOR_TABLE_REL, "color.json 不存在"))
    else:
        try:
            table = json.loads(color_path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            findings.append(
                Finding("accent-color-table", HARMONY_COLOR_TABLE_REL, f"JSON 解析失败：{exc}")
            )
        else:
            findings.extend(check_color_table(table))

    ets_root = root / HARMONY_ETS_REL
    if not ets_root.is_dir():
        findings.append(Finding("image-fill-color", HARMONY_ETS_REL, "ArkTS 源码目录不存在"))
    else:
        for source in sorted(ets_root.rglob("*.ets")):
            rel = source.relative_to(root).as_posix()
            text = source.read_text(encoding="utf-8")
            findings.extend(check_ets_source(rel, text, tintable_icons))
            findings.extend(check_blue_accent_source(rel, text))
            findings.extend(check_theme_single_source(rel, text))

    adapter = root / THEME_ADAPTER_REL
    if not adapter.is_file():
        findings.append(Finding("theme-single-source", THEME_ADAPTER_REL, "主题适配器不存在"))
    elif MONOCHROME_ENTRY not in strip_comments(adapter.read_text(encoding="utf-8")):
        findings.append(
            Finding(
                "theme-single-source",
                THEME_ADAPTER_REL,
                f"没有调用 {MONOCHROME_ENTRY}(...)：深浅色切换不会下发应用级单色强调色",
            )
        )

    return findings


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="HarmonyOS 单色强调色 / 深色图标守卫（Issue #784）")
    parser.add_argument("root", nargs="?", default=".", help="仓库根目录")
    args = parser.parse_args()
    root = Path(args.root).resolve()

    findings = sorted(set(check_repository(root)), key=lambda f: (f.rule, f.where, f.message))

    print("=" * 60)
    print("HarmonyOS 单色强调色 / 深色模式图标守卫（Issue #784）")
    print("=" * 60)
    if not findings:
        print("ALL PASS")
        return 0
    for finding in findings:
        print(f"[FAIL] {finding.rule}: {finding.where}")
        print(f"       {finding.message}")
    print()
    print(f"共 {len(findings)} 处违反单色主题 / 深色图标约束")
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
