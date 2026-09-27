#!/usr/bin/env python3
"""test_check_harmony_monochrome_theme.py — check_harmony_monochrome_theme.py 的单元测试。

Issue #784 最后三轮（评论 5858361456 / 5858404437 / 5858437476）反复卡在同一处：
自定义 SVG 写死黑色 + `Image` 没有 `.fillColor()`，深色模式下右上角图标不反色。
这里把每个方向的样例都钉死：

- 当前仓库必须通过；
- `fill="#000000"`、缺 `.fillColor()` 必须被判失败；
- 星图节点 kind 的分类色板（蓝色 mark）不是强调色，不能误报；
- 强调色资源名只看首段，`text_primary` 这类中性名不能误报；
- 应用级主题入口 `applySujianMonochromeTheme` 从适配器里消失必须被判失败。
"""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("check_harmony_monochrome_theme.py")
SPEC = importlib.util.spec_from_file_location("check_harmony_monochrome_theme", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)

REPO_ROOT = Path(__file__).resolve().parents[1]

TINTABLE_ICONS = {"ic_search", "ic_settings", "ic_refresh", "ic_star"}

SEARCH_SVG_OK = (
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">\n'
    '  <path d="M1 2h3" fill="currentColor"/>\n'
    "</svg>\n"
)


def rules(findings: list[object]) -> list[str]:
    return [finding.rule for finding in findings]


class RepositoryGuardTests(unittest.TestCase):
    def test_tracked_repository_passes(self) -> None:
        """仓库里真正提交的 Harmony 代码必须通过全部单色主题约束。"""
        self.assertEqual(MODULE.check_repository(REPO_ROOT), [])

    def test_tracked_color_table_has_no_accent_resources(self) -> None:
        table = json.loads((REPO_ROOT / MODULE.HARMONY_COLOR_TABLE_REL).read_text(encoding="utf-8"))
        names = [entry["name"] for entry in table["color"]]
        self.assertNotIn("primary_color", names)
        self.assertNotIn("secondary_color", names)
        self.assertIn(MODULE.START_WINDOW_COLOR_NAME, names)

    def test_tracked_svg_icons_are_tintable(self) -> None:
        media_dir = REPO_ROOT / MODULE.HARMONY_MEDIA_REL
        svgs = sorted(media_dir.glob("*.svg"))
        self.assertTrue(svgs, "Harmony 客户端应当有自定义 SVG 图标")
        for svg in svgs:
            with self.subTest(svg=svg.name):
                self.assertEqual(MODULE.check_svg(svg.name, svg.read_text(encoding="utf-8")), [])


class SvgTintableTests(unittest.TestCase):
    def test_current_color_passes(self) -> None:
        self.assertEqual(MODULE.check_svg("ic_search.svg", SEARCH_SVG_OK), [])

    def test_fixed_hex_fails(self) -> None:
        """评论 5858361456 的原始回归：写死 #000000。"""
        for fill in ('fill="#000000"', "fill='#000000'", 'fill: #000000;'):
            with self.subTest(fill=fill):
                self.assertIn(
                    "svg-tintable",
                    rules(MODULE.check_svg("ic_settings.svg", f'<svg><path d="M0 0h1" {fill}/></svg>')),
                )

    def test_style_fill_hex_fails(self) -> None:
        findings = MODULE.check_svg(
            "ic_settings.svg", '<svg><path style="fill:#fafafa;stroke:none"/></svg>'
        )
        self.assertIn("svg-tintable", rules(findings))

    def test_named_color_fails(self) -> None:
        findings = MODULE.check_svg("ic_star.svg", '<svg><path fill="black"/></svg>')
        self.assertIn("svg-tintable", rules(findings))

    def test_none_and_opacity_are_allowed(self) -> None:
        findings = MODULE.check_svg(
            "ic_refresh.svg", '<svg><path fill="none"/><path fill-opacity="0.4"/></svg>'
        )
        self.assertEqual(findings, [])


class ImageFillColorTests(unittest.TestCase):
    PATH = "apps/harmony/entry/src/main/ets/feature/project/ui/HomeScreen.ets"

    def test_fill_color_passes(self) -> None:
        text = (
            "            Image($r('app.media.ic_star'))\n"
            "              .width(36)\n"
            "              .height(36)\n"
            "              .fillColor($r('sys.color.icon_primary'))\n"
        )
        self.assertEqual(MODULE.check_ets_source(self.PATH, text, TINTABLE_ICONS), [])

    def test_missing_fill_color_fails(self) -> None:
        text = "Image($r('app.media.ic_search'))\n  .width(24)\n  .height(24)\n"
        findings = MODULE.check_ets_source(self.PATH, text, TINTABLE_ICONS)
        self.assertEqual(rules(findings), ["image-fill-color"])
        self.assertEqual(findings[0].where, f"{self.PATH}:1")

    def test_multiline_image_call_is_scanned(self) -> None:
        text = "Image(\n  $r('app.media.ic_settings')\n)\n  .width(24)\n"
        self.assertIn("image-fill-color", rules(MODULE.check_ets_source(self.PATH, text, TINTABLE_ICONS)))

    def test_header_helper_image_requires_fill_color(self) -> None:
        """WritingScreen API<23 自定义标题栏的页头动作图标。"""
        text = (
            "Image(this.getHeaderActionIcon(slot.role))\n"
            "  .width(24)\n"
            "  .height(24)\n"
        )
        self.assertIn("image-fill-color", rules(MODULE.check_ets_source(self.PATH, text, TINTABLE_ICONS)))
        tinted = text + "  .fillColor($r('sys.color.icon_primary'))\n"
        self.assertEqual(MODULE.check_ets_source(self.PATH, tinted, TINTABLE_ICONS), [])

    def test_non_tintable_resource_is_ignored(self) -> None:
        """PNG / 非 SVG 资源不在可着色规则范围内。"""
        text = "Image($r('app.media.startIcon'))\n  .width(48)\n"
        self.assertEqual(MODULE.check_ets_source(self.PATH, text, TINTABLE_ICONS), [])

    def test_gap_between_statements_does_not_leak_fill_color(self) -> None:
        """前一个 Image 的 fillColor 不能算到后一个 Image 头上。"""
        text = (
            "Image($r('app.media.ic_star'))\n"
            "  .fillColor($r('sys.color.icon_primary'))\n"
            "\n"
            "Image($r('app.media.ic_search'))\n"
            "  .width(24)\n"
        )
        findings = MODULE.check_ets_source(self.PATH, text, TINTABLE_ICONS)
        self.assertEqual(rules(findings), ["image-fill-color"])
        self.assertEqual(findings[0].where, f"{self.PATH}:4")

    def test_commented_out_image_is_ignored(self) -> None:
        text = "// Image($r('app.media.ic_search'))\n  // .width(24)\n"
        self.assertEqual(MODULE.check_ets_source(self.PATH, text, TINTABLE_ICONS), [])


class AccentColorTableTests(unittest.TestCase):
    def test_accent_names_fail(self) -> None:
        table = {"color": [{"name": "primary_color", "value": "#4A90D9"}]}
        findings = MODULE.check_color_table(table)
        self.assertIn("accent-color-table", rules(findings))

    def test_neutral_legacy_names_are_allowed(self) -> None:
        table = {
            "color": [
                {"name": MODULE.START_WINDOW_COLOR_NAME, "value": "#FFFFFF"},
                {"name": "text_primary", "value": "#333333"},
                {"name": "text_secondary", "value": "#666666"},
            ]
        }
        self.assertEqual(MODULE.check_color_table(table), [])

    def test_missing_start_window_background_fails(self) -> None:
        findings = MODULE.check_color_table({"color": [{"name": "surface_color", "value": "#FFFFFF"}]})
        self.assertIn("accent-color-table", rules(findings))

    def test_invalid_shape_fails(self) -> None:
        self.assertIn("accent-color-table", rules(MODULE.check_color_table({"colors": []})))


class BlueAccentSourceTests(unittest.TestCase):
    PAGE = "apps/harmony/entry/src/main/ets/feature/project/ui/HomeScreen.ets"
    THEME = "apps/harmony/entry/src/main/ets/ui/theme/HarmonyThemeAdapter.ets"

    def test_blue_system_tokens_fail(self) -> None:
        for snippet in (".backgroundColor(Color.Blue)", ".backgroundColor($r('sys.color.brand'))",
                        ".backgroundColor($r('sys.color.activated'))"):
            with self.subTest(snippet=snippet):
                self.assertIn("no-blue-accent-source", rules(MODULE.check_blue_accent_source(self.PAGE, snippet)))

    def test_retired_accent_literal_in_theme_layer_fails(self) -> None:
        findings = MODULE.check_blue_accent_source(self.THEME, "primary: '#4A90D9',")
        self.assertIn("no-blue-accent-source", rules(findings))

    def test_categorical_mark_color_is_not_an_accent(self) -> None:
        """星图节点 kind 色板（Character 蓝 mark）是图表分类色，不是应用强调色。"""
        star_map = "apps/harmony/entry/src/main/ets/feature/starmap/ui/StarMapScreen.ets"
        text = "const kindColors: Record<string, string> = { 'Character': '#4A90D9' }\n"
        self.assertEqual(MODULE.check_blue_accent_source(star_map, text), [])


class ThemeSingleSourceTests(unittest.TestCase):
    PAGE = "apps/harmony/entry/src/main/ets/feature/settings/ui/SettingsScreen.ets"
    THEME = MODULE.MONOCHROME_THEME_REL

    def test_hard_black_white_in_page_fails(self) -> None:
        for snippet in (".backgroundColor(Color.Black)", ".backgroundColor(Color.White)"):
            with self.subTest(snippet=snippet):
                self.assertIn("theme-single-source", rules(MODULE.check_theme_single_source(self.PAGE, snippet)))

    def test_hard_black_white_in_monochrome_theme_passes(self) -> None:
        text = "const emphasize: ResourceColor = isDark ? Color.White : Color.Black\n"
        self.assertEqual(MODULE.check_theme_single_source(self.THEME, text), [])

    def test_widget_accent_override_in_page_fails(self) -> None:
        text = "Toggle({ type: ToggleType.Switch, isOn: true })\n  .selectedColor(Color.Blue)\n"
        self.assertIn("theme-single-source", rules(MODULE.check_theme_single_source(self.PAGE, text)))

    def test_commented_out_snippet_is_ignored(self) -> None:
        text = "// .backgroundColor(Color.Black)\n"
        self.assertEqual(MODULE.check_theme_single_source(self.PAGE, text), [])


class RepositoryLayoutTests(unittest.TestCase):
    def _write_repo(self, root: Path, *, svg: str, color_names: list[str], page_fill_color: bool,
                    adapter_calls_entry: bool, page_extra: str = "") -> None:
        media = root / MODULE.HARMONY_MEDIA_REL
        media.mkdir(parents=True, exist_ok=True)
        (media / "ic_search.svg").write_text(svg, encoding="utf-8")

        color_path = root / MODULE.HARMONY_COLOR_TABLE_REL
        color_path.parent.mkdir(parents=True, exist_ok=True)
        color_path.write_text(
            json.dumps({"color": [{"name": name, "value": "#FFFFFF"} for name in color_names]}),
            encoding="utf-8",
        )

        page = root / MODULE.HARMONY_ETS_REL / "feature/project/ui/HomeScreen.ets"
        page.parent.mkdir(parents=True, exist_ok=True)
        chain = ".fillColor($r('sys.color.icon_primary'))\n" if page_fill_color else ".width(24)\n"
        page.write_text(f"Image($r('app.media.ic_search'))\n{chain}{page_extra}", encoding="utf-8")

        adapter = root / MODULE.THEME_ADAPTER_REL
        adapter.parent.mkdir(parents=True, exist_ok=True)
        call = f"  {MODULE.MONOCHROME_ENTRY}(isDark)\n" if adapter_calls_entry else ""
        adapter.write_text(f"export class HarmonyThemeAdapter {{\n{call}}}\n", encoding="utf-8")

        theme = root / MODULE.MONOCHROME_THEME_REL
        theme.write_text(
            f"export function {MODULE.MONOCHROME_ENTRY}(isDark: boolean): void {{\n"
            "  const emphasize = isDark ? Color.White : Color.Black\n"
            "}\n",
            encoding="utf-8",
        )

    def _clean_repo(self, root: Path) -> None:
        self._write_repo(
            root,
            svg=SEARCH_SVG_OK,
            color_names=[MODULE.START_WINDOW_COLOR_NAME, "text_primary"],
            page_fill_color=True,
            adapter_calls_entry=True,
        )

    def test_clean_layered_repo_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self._clean_repo(root)
            self.assertEqual(MODULE.check_repository(root), [])

    def test_fixed_svg_fill_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self._clean_repo(root)
            (root / MODULE.HARMONY_MEDIA_REL / "ic_search.svg").write_text(
                '<svg><path fill="#000000"/></svg>', encoding="utf-8"
            )
            self.assertIn("svg-tintable", rules(MODULE.check_repository(root)))

    def test_missing_image_fill_color_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self._write_repo(
                root,
                svg=SEARCH_SVG_OK,
                color_names=[MODULE.START_WINDOW_COLOR_NAME],
                page_fill_color=False,
                adapter_calls_entry=True,
            )
            self.assertIn("image-fill-color", rules(MODULE.check_repository(root)))

    def test_accent_resource_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self._write_repo(
                root,
                svg=SEARCH_SVG_OK,
                color_names=[MODULE.START_WINDOW_COLOR_NAME, "secondary_color"],
                page_fill_color=True,
                adapter_calls_entry=True,
            )
            self.assertIn("accent-color-table", rules(MODULE.check_repository(root)))

    def test_missing_theme_entry_call_is_reported(self) -> None:
        """评论 5856805657 第1节：深浅色切换必须经过唯一主题刷新链。"""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self._write_repo(
                root,
                svg=SEARCH_SVG_OK,
                color_names=[MODULE.START_WINDOW_COLOR_NAME],
                page_fill_color=True,
                adapter_calls_entry=False,
            )
            self.assertIn("theme-single-source", rules(MODULE.check_repository(root)))

    def test_page_hardcoded_emphasize_color_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self._write_repo(
                root,
                svg=SEARCH_SVG_OK,
                color_names=[MODULE.START_WINDOW_COLOR_NAME],
                page_fill_color=True,
                adapter_calls_entry=True,
                page_extra="      .backgroundColor(Color.Black)\n",
            )
            self.assertIn("theme-single-source", rules(MODULE.check_repository(root)))

    def test_missing_media_dir_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            self.assertIn("svg-tintable", rules(MODULE.check_repository(Path(tmp))))


if __name__ == "__main__":
    unittest.main(verbosity=2)
