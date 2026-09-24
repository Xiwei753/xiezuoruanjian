#!/usr/bin/env python3
"""#609 三：Android Adaptive Icon 资源链生成测试。

守护 scripts/generate_icons.py 的行为契约：
- 自适应图标前景按 5 个密度桶输出到 mipmap-*，尺寸 108/162/216/324/432；
- 不再生成 drawable/ic_launcher_foreground.png（旧混合资源链导致桌面发糊）；
- fit_foreground_to_safe_zone 把前景内容缩进 66×66 dp 安全区并居中；
- 已提交 res 目录结构必须与生成器输出一致（防回归）。
"""

from __future__ import annotations

import importlib.util
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

from PIL import Image

MODULE_PATH = Path(__file__).resolve().parents[1] / "scripts" / "generate_icons.py"
SPEC = importlib.util.spec_from_file_location("generate_icons", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)

REPO_ROOT = Path(__file__).resolve().parents[1]
RES_ROOT = REPO_ROOT / "apps" / "android" / "app" / "src" / "main" / "res"
SOURCE_DIR = REPO_ROOT / "assets" / "brand" / "icon" / "source"

FOREGROUND_DENSITIES = {
    "mipmap-mdpi": 108,
    "mipmap-hdpi": 162,
    "mipmap-xhdpi": 216,
    "mipmap-xxhdpi": 324,
    "mipmap-xxxhdpi": 432,
}
LEGACY_BITMAP_DENSITIES = {
    "mipmap-mdpi": 48,
    "mipmap-hdpi": 72,
    "mipmap-xhdpi": 96,
    "mipmap-xxhdpi": 144,
    "mipmap-xxxhdpi": 192,
}

HARMONY_APP_ICON = REPO_ROOT / "apps" / "harmony" / "AppScope" / "resources" / "base" / "media" / "app_icon.png"
HARMONY_ENTRY_MEDIA = REPO_ROOT / "apps" / "harmony" / "entry" / "src" / "main" / "resources" / "base" / "media"


def _content_bbox(image: Image.Image):
    """不透明内容（alpha > 24）的包围盒，与生成器内部判定一致。"""
    alpha = image.split()[3]
    return alpha.point(lambda v: 255 if v > 24 else 0).getbbox()


class FitForegroundToSafeZoneTests(unittest.TestCase):
    """fit_foreground_to_safe_zone 纯函数正反测试（#609 三 安全区）。"""

    def test_content_filling_canvas_is_shrunk_into_safe_zone(self):
        # 反：内容铺满整个 1024 画布，明显超出 66dp 安全区
        canvas = Image.new("RGBA", (1024, 1024), (255, 0, 0, 255))
        result = MODULE.fit_foreground_to_safe_zone(canvas)
        bbox = _content_bbox(result)
        self.assertIsNotNone(bbox, "缩放后应仍有内容")
        content_w = bbox[2] - bbox[0]
        content_h = bbox[3] - bbox[1]
        safe = 66.0 / 108.0 * 1024  # ≈ 628px
        self.assertLessEqual(content_w, safe, "内容宽度必须落入 66dp 安全区")
        self.assertLessEqual(content_h, safe, "内容高度必须落入 66dp 安全区")

    def test_already_safe_content_stays_within_safe_zone(self):
        # 正：内容已在中心 200×200（远小于安全区），不放大
        canvas = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
        canvas.paste((0, 255, 0, 255), (412, 412, 612, 612))
        result = MODULE.fit_foreground_to_safe_zone(canvas)
        bbox = _content_bbox(result)
        self.assertIsNotNone(bbox)
        content_w = bbox[2] - bbox[0]
        content_h = bbox[3] - bbox[1]
        safe = 66.0 / 108.0 * 1024
        self.assertLessEqual(content_w, safe)
        self.assertLessEqual(content_h, safe)

    def test_empty_image_is_noop(self):
        canvas = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
        result = MODULE.fit_foreground_to_safe_zone(canvas)
        self.assertIsNone(_content_bbox(result))

    def test_content_is_centered_after_fit(self):
        # 非对称内容（顶部条带）缩放后应居中
        canvas = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
        canvas.paste((0, 0, 255, 255), (0, 0, 1024, 400))
        result = MODULE.fit_foreground_to_safe_zone(canvas)
        bbox = _content_bbox(result)
        self.assertIsNotNone(bbox)
        cx = (bbox[0] + bbox[2]) / 2
        cy = (bbox[1] + bbox[3]) / 2
        self.assertAlmostEqual(cx, 512, delta=3, msg="内容应水平居中")
        self.assertAlmostEqual(cy, 512, delta=3, msg="内容应垂直居中")


class GenerateAndroidIconStructureTests(unittest.TestCase):
    """generate_android 输出结构测试（写入临时目录，不触碰已提交资源）。"""

    def setUp(self):
        self._tmp = tempfile.mkdtemp(prefix="sujian_icons_test_")
        tmp_root = Path(self._tmp)
        tmp_source = tmp_root / "assets" / "brand" / "icon" / "source"
        tmp_source.mkdir(parents=True)
        for f in SOURCE_DIR.iterdir():
            shutil.copyfile(f, tmp_source / f.name)
        self._orig = {
            "ROOT": MODULE.ROOT,
            "SOURCE": MODULE.SOURCE,
            "FULL_SVG": MODULE.FULL_SVG,
            "FOREGROUND_PNG": MODULE.FOREGROUND_PNG,
            "FULL_1024": MODULE.FULL_1024,
            "FULL_512": MODULE.FULL_512,
        }
        MODULE.ROOT = tmp_root
        MODULE.SOURCE = tmp_source
        MODULE.FULL_SVG = tmp_source / "sujian_icon.svg"
        MODULE.FOREGROUND_PNG = tmp_source / "sujian_icon_foreground_1024.png"
        MODULE.FULL_1024 = tmp_source / "sujian_icon_1024.png"
        MODULE.FULL_512 = tmp_source / "sujian_icon_512.png"
        self.tmp_res = tmp_root / "apps" / "android" / "app" / "src" / "main" / "res"

    def tearDown(self):
        for key, value in self._orig.items():
            setattr(MODULE, key, value)
        shutil.rmtree(self._tmp, ignore_errors=True)

    def test_foreground_pngs_in_five_mipmap_densities_at_exact_sizes(self):
        MODULE.generate_android()
        for directory, size in FOREGROUND_DENSITIES.items():
            path = self.tmp_res / directory / "ic_launcher_foreground.png"
            self.assertTrue(path.exists(), f"缺少 {directory}/ic_launcher_foreground.png")
            with Image.open(path) as im:
                self.assertEqual(
                    (size, size),
                    im.size,
                    f"{directory} 前景尺寸应为 {size}×{size}",
                )

    def test_no_drawable_ic_launcher_foreground_png(self):
        # 反：不得生成 drawable/ic_launcher_foreground.png（旧混合资源链导致桌面发糊）
        MODULE.generate_android()
        stray = self.tmp_res / "drawable" / "ic_launcher_foreground.png"
        self.assertFalse(
            stray.exists(),
            "不得生成 drawable/ic_launcher_foreground.png",
        )

    def test_legacy_bitmap_icons_generated_at_correct_sizes(self):
        MODULE.generate_android()
        for directory, size in LEGACY_BITMAP_DENSITIES.items():
            with Image.open(self.tmp_res / directory / "ic_launcher.png") as im:
                self.assertEqual((size, size), im.size)
            with Image.open(self.tmp_res / directory / "ic_launcher_round.png") as im:
                self.assertEqual((size, size), im.size)

    def test_foreground_content_within_safe_zone(self):
        # 生成的每个密度前景内容必须落在 66×66 dp 安全区内
        MODULE.generate_android()
        for directory, size in FOREGROUND_DENSITIES.items():
            with Image.open(self.tmp_res / directory / "ic_launcher_foreground.png") as im:
                bbox = _content_bbox(im)
                if bbox is None:
                    continue
                content_w = bbox[2] - bbox[0]
                content_h = bbox[3] - bbox[1]
                safe = 66.0 / 108.0 * size
                self.assertLessEqual(
                    content_w,
                    safe,
                    f"{directory} 前景内容宽度超出 66dp 安全区",
                )
                self.assertLessEqual(
                    content_h,
                    safe,
                    f"{directory} 前景内容高度超出 66dp 安全区",
                )


class CommittedAndroidIconResourceGuardTests(unittest.TestCase):
    """已提交 res 目录结构守卫（防回归重新引入 #609 三 修复的问题）。"""

    def test_committed_foreground_pngs_exist_at_exact_sizes(self):
        for directory, size in FOREGROUND_DENSITIES.items():
            path = RES_ROOT / directory / "ic_launcher_foreground.png"
            self.assertTrue(
                path.exists(),
                f"已提交资源缺少 {directory}/ic_launcher_foreground.png",
            )
            with Image.open(path) as im:
                self.assertEqual((size, size), im.size)

    def test_no_committed_drawable_ic_launcher_foreground_png(self):
        stray = RES_ROOT / "drawable" / "ic_launcher_foreground.png"
        self.assertFalse(
            stray.exists(),
            "已提交 drawable/ic_launcher_foreground.png 必须删除",
        )

    def test_adaptive_icon_xml_references_mipmap_foreground_and_drawable_background(self):
        xml = (RES_ROOT / "mipmap-anydpi-v26" / "ic_launcher.xml").read_text()
        self.assertIn("@mipmap/ic_launcher_foreground", xml)
        self.assertIn("@drawable/ic_launcher_background", xml)

    def test_adaptive_icon_round_xml_exists(self):
        self.assertTrue(
            (RES_ROOT / "mipmap-anydpi-v26" / "ic_launcher_round.xml").exists()
        )

    def test_background_layer_exists(self):
        self.assertTrue(
            (RES_ROOT / "drawable" / "ic_launcher_background.xml").exists(),
            "自适应图标背景层 ic_launcher_background.xml 必须存在",
        )


class GenerateHarmonyIconStructureTests(unittest.TestCase):
    """generate_harmony 输出结构测试（写入临时目录，不触碰已提交资源）。"""

    def setUp(self):
        self._tmp = tempfile.mkdtemp(prefix="sujian_icons_test_")
        tmp_root = Path(self._tmp)
        tmp_source = tmp_root / "assets" / "brand" / "icon" / "source"
        tmp_source.mkdir(parents=True)
        for f in SOURCE_DIR.iterdir():
            shutil.copyfile(f, tmp_source / f.name)
        self._orig = {
            "ROOT": MODULE.ROOT,
            "SOURCE": MODULE.SOURCE,
            "FULL_SVG": MODULE.FULL_SVG,
            "FOREGROUND_PNG": MODULE.FOREGROUND_PNG,
            "FULL_1024": MODULE.FULL_1024,
            "FULL_512": MODULE.FULL_512,
        }
        MODULE.ROOT = tmp_root
        MODULE.SOURCE = tmp_source
        MODULE.FULL_SVG = tmp_source / "sujian_icon.svg"
        MODULE.FOREGROUND_PNG = tmp_source / "sujian_icon_foreground_1024.png"
        MODULE.FULL_1024 = tmp_source / "sujian_icon_1024.png"
        MODULE.FULL_512 = tmp_source / "sujian_icon_512.png"
        self.tmp_app_scope_media = tmp_root / "apps" / "harmony" / "AppScope" / "resources" / "base" / "media"
        self.tmp_entry_media = tmp_root / "apps" / "harmony" / "entry" / "src" / "main" / "resources" / "base" / "media"

    def tearDown(self):
        for key, value in self._orig.items():
            setattr(MODULE, key, value)
        shutil.rmtree(self._tmp, ignore_errors=True)

    def test_app_icon_generated_at_512(self):
        MODULE.generate_harmony()
        path = self.tmp_app_scope_media / "app_icon.png"
        self.assertTrue(path.exists(), "缺少 app_icon.png")
        with Image.open(path) as im:
            self.assertEqual((512, 512), im.size, "app_icon.png 应为 512×512")

    def test_start_icon_generated_at_512(self):
        MODULE.generate_harmony()
        path = self.tmp_entry_media / "startIcon.png"
        self.assertTrue(path.exists(), "缺少 startIcon.png")
        with Image.open(path) as im:
            self.assertEqual((512, 512), im.size, "startIcon.png 应为 512×512")

    def test_layered_foreground_generated_at_216_with_alpha(self):
        MODULE.generate_harmony()
        path = self.tmp_entry_media / "layered_image_foreground.png"
        self.assertTrue(path.exists(), "缺少 layered_image_foreground.png")
        with Image.open(path) as im:
            self.assertEqual((216, 216), im.size, "前景应为 216×216")
            self.assertEqual("RGBA", im.mode, "前景应保留 RGBA 透明通道")
            # 存在透明像素（alpha < 255）
            a_min = im.getchannel("A").getextrema()[0]
            self.assertLess(a_min, 255, "前景应存在透明像素")

    def test_layered_foreground_not_solid_color(self):
        MODULE.generate_harmony()
        path = self.tmp_entry_media / "layered_image_foreground.png"
        with Image.open(path) as im:
            self.assertGreater(
                len(set(im.convert("RGBA").getdata())),
                1,
                "前景不得为占位纯色块",
            )

    def test_layered_background_generated_at_216_white(self):
        MODULE.generate_harmony()
        path = self.tmp_entry_media / "layered_image_background.png"
        self.assertTrue(path.exists(), "缺少 layered_image_background.png")
        with Image.open(path) as im:
            self.assertEqual((216, 216), im.size, "背景应为 216×216")
            self.assertEqual(
                {(255, 255, 255)},
                set(im.getdata()),
                "背景应为纯白 #FFFFFF",
            )

    def test_app_icon_not_solid_color(self):
        MODULE.generate_harmony()
        path = self.tmp_app_scope_media / "app_icon.png"
        with Image.open(path) as im:
            self.assertGreater(
                len(set(im.convert("RGBA").getdata())),
                1,
                "app_icon 不得为占位纯色块",
            )


class CommittedHarmonyIconResourceGuardTests(unittest.TestCase):
    """已提交 Harmony 图标资源守卫（防回归到 48×48 占位纯色块，Issue #752）。"""

    def test_committed_app_icon_exists_at_512_not_solid(self):
        self.assertTrue(HARMONY_APP_ICON.exists(), "已提交 app_icon.png 缺失")
        with Image.open(HARMONY_APP_ICON) as im:
            self.assertEqual((512, 512), im.size, "app_icon.png 应为 512×512")
            self.assertGreater(
                len(set(im.convert("RGBA").getdata())),
                1,
                "app_icon.png 不得为占位纯色块",
            )

    def test_committed_start_icon_exists_at_512_not_solid(self):
        path = HARMONY_ENTRY_MEDIA / "startIcon.png"
        self.assertTrue(path.exists(), "已提交 startIcon.png 缺失")
        with Image.open(path) as im:
            self.assertEqual((512, 512), im.size, "startIcon.png 应为 512×512")
            self.assertGreater(
                len(set(im.convert("RGBA").getdata())),
                1,
                "startIcon.png 不得为占位纯色块",
            )

    def test_committed_layered_foreground_exists_at_216_with_alpha_not_solid(self):
        path = HARMONY_ENTRY_MEDIA / "layered_image_foreground.png"
        self.assertTrue(path.exists(), "已提交 layered_image_foreground.png 缺失")
        with Image.open(path) as im:
            self.assertEqual((216, 216), im.size, "前景应为 216×216")
            self.assertEqual("RGBA", im.mode, "前景应保留 RGBA 透明通道")
            a_min = im.getchannel("A").getextrema()[0]
            self.assertLess(a_min, 255, "前景应存在透明像素")
            self.assertGreater(
                len(set(im.convert("RGBA").getdata())),
                1,
                "前景不得为占位纯色块",
            )

    def test_committed_layered_background_exists_at_216_white(self):
        path = HARMONY_ENTRY_MEDIA / "layered_image_background.png"
        self.assertTrue(path.exists(), "已提交 layered_image_background.png 缺失")
        with Image.open(path) as im:
            self.assertEqual((216, 216), im.size, "背景应为 216×216")
            self.assertEqual(
                {(255, 255, 255)},
                set(im.getdata()),
                "背景应为纯白 #FFFFFF",
            )

    def test_layered_image_json_references_correct_resources(self):
        content = (HARMONY_ENTRY_MEDIA / "layered_image.json").read_text()
        self.assertIn("$media:layered_image_background", content)
        self.assertIn("$media:layered_image_foreground", content)

    def test_app_json5_icon_reference_unchanged(self):
        content = (REPO_ROOT / "apps" / "harmony" / "AppScope" / "app.json5").read_text()
        self.assertIn("$media:app_icon", content)

    def test_module_json5_icon_references_unchanged(self):
        content = (REPO_ROOT / "apps" / "harmony" / "entry" / "src" / "main" / "module.json5").read_text()
        self.assertIn("$media:layered_image", content)
        self.assertIn("$media:startIcon", content)


if __name__ == "__main__":
    unittest.main()
