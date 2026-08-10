#!/usr/bin/env python3
"""Generate platform icon assets from assets/brand/icon/source."""

from pathlib import Path
import shutil

try:
    from PIL import Image
except ImportError as exc:
    raise SystemExit(
        "Pillow is required to generate icons. Install python3-pillow or Pillow."
    ) from exc


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "assets" / "brand" / "icon" / "source"

FULL_SVG = SOURCE / "sujian_icon.svg"
FOREGROUND_PNG = SOURCE / "sujian_icon_foreground_1024.png"
FULL_1024 = SOURCE / "sujian_icon_1024.png"
FULL_512 = SOURCE / "sujian_icon_512.png"


def ensure_sources() -> None:
    required = [FULL_SVG, FOREGROUND_PNG, FULL_1024, FULL_512]
    missing = [str(path.relative_to(ROOT)) for path in required if not path.exists()]
    if missing:
        raise SystemExit("Missing icon source files: " + ", ".join(missing))


def copy_file(src: Path, dst: Path) -> None:
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(src, dst)


def resize_png(src: Path, dst: Path, size: int) -> None:
    dst.parent.mkdir(parents=True, exist_ok=True)
    with Image.open(src) as image:
        image = image.convert("RGBA").resize((size, size), Image.Resampling.LANCZOS)
        image.save(dst, "PNG")


def fit_foreground_to_safe_zone(image: Image.Image) -> Image.Image:
    """把自适应图标前景内容缩放到 66×66 dp 安全区内并居中（Issue #609 三）。

    Android 启动器按 108×108 dp 图层裁切（圆/圆角方掩码），前景重要图形必须
    落在 66×66 dp 安全区内才不会被不同启动器裁剪不一致。母版
    sujian_icon_foreground_1024.png 的斜笺图形延伸到画布边缘且不在画布中心，
    这里按不透明内容包围盒裁剪后等比缩放（只缩小不放大）再居中，保证生成
    的全部密度资源合规。0.94 余量吸收缩小插值扩散的边缘半透明像素。
    """
    safe_fraction = 66.0 / 108.0 * 0.94
    alpha = image.split()[3]
    content_bbox = alpha.point(lambda v: 255 if v > 24 else 0).getbbox()
    if content_bbox is None:
        return image
    content_w = content_bbox[2] - content_bbox[0]
    content_h = content_bbox[3] - content_bbox[1]
    target = int(round(safe_fraction * image.width))
    scale = min(1.0, target / max(content_w, content_h))
    new_w = max(1, int(round(content_w * scale)))
    new_h = max(1, int(round(content_h * scale)))
    cropped = image.crop(content_bbox)
    resized = cropped.resize((new_w, new_h), Image.Resampling.LANCZOS)
    canvas = Image.new("RGBA", image.size, (0, 0, 0, 0))
    canvas.paste(
        resized,
        ((image.width - new_w) // 2, (image.height - new_h) // 2),
    )
    return canvas


def resize_foreground_png(src: Path, dst: Path, size: int) -> None:
    dst.parent.mkdir(parents=True, exist_ok=True)
    with Image.open(src) as image:
        image = image.convert("RGBA").resize((size, size), Image.Resampling.LANCZOS)
        # 在最终密度尺寸上做安全区适配：缩放插值会扩散边缘半透明像素，
        # 按最终尺寸测量包围盒才能保证出货资源严格落在 66dp 安全区内。
        image = fit_foreground_to_safe_zone(image)
        image.save(dst, "PNG")


def generate_android() -> None:
    res = ROOT / "apps" / "android" / "app" / "src" / "main" / "res"

    # 自适应图标前景层：按密度桶输出，XML 引用 @mipmap/ic_launcher_foreground。
    # 自适应图标按 108×108 dp 图层设计（mdpi 1x = 108px），前景内容经
    # fit_foreground_to_safe_zone 缩放到 66×66 dp 安全区内；不得先压缩到
    # 192px 再让系统当自适应图标放大（Issue #609 三）。
    foreground_densities = {
        "mipmap-mdpi": 108,
        "mipmap-hdpi": 162,
        "mipmap-xhdpi": 216,
        "mipmap-xxhdpi": 324,
        "mipmap-xxxhdpi": 432,
    }
    for directory, size in foreground_densities.items():
        resize_foreground_png(FOREGROUND_PNG, res / directory / "ic_launcher_foreground.png", size)

    # 旧式（API < 26）启动器位图：Launcher 在无自适应图标时按 48/72/96/144/192 px 选用。
    densities = {
        "mipmap-mdpi": 48,
        "mipmap-hdpi": 72,
        "mipmap-xhdpi": 96,
        "mipmap-xxhdpi": 144,
        "mipmap-xxxhdpi": 192,
    }
    for directory, size in densities.items():
        resize_png(FULL_1024, res / directory / "ic_launcher.png", size)
        resize_png(FULL_1024, res / directory / "ic_launcher_round.png", size)


def generate_linux() -> None:
    copy_file(FULL_SVG, ROOT / "apps" / "Linux_qt" / "resources" / "icons" / "sujian.svg")
    copy_file(FULL_SVG, ROOT / "packaging" / "linux" / "icons" / "hicolor" / "scalable" / "apps" / "sujian.svg")
    resize_png(FULL_1024, ROOT / "packaging" / "linux" / "icons" / "hicolor" / "256x256" / "apps" / "sujian.png", 256)


def generate_packaging() -> None:
    copy_file(FULL_512, ROOT / "packaging" / "google-play" / "icon-512.png")
    copy_file(FULL_1024, ROOT / "packaging" / "apple" / "app-icon-1024.png")
    copy_file(FULL_512, ROOT / "packaging" / "web" / "icon-512.png")
    resize_png(FULL_1024, ROOT / "packaging" / "web" / "icon-192.png", 192)

def main() -> None:
    ensure_sources()
    generate_android()
    generate_linux()
    generate_packaging()


if __name__ == "__main__":
    main()
