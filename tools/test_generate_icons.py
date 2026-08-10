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


if __name__ == "__main__":
    unittest.main()
