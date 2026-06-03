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


def generate_android() -> None:
    res = ROOT / "apps" / "android" / "app" / "src" / "main" / "res"
    resize_png(FOREGROUND_PNG, res / "drawable" / "ic_launcher_foreground.png", 432)

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
    copy_file(FULL_SVG, ROOT / "apps" / "desktop" / "resources" / "icons" / "sujian.svg")
    copy_file(FULL_SVG, ROOT / "packaging" / "linux" / "icons" / "hicolor" / "scalable" / "apps" / "sujian.svg")
    resize_png(FULL_1024, ROOT / "packaging" / "linux" / "icons" / "hicolor" / "256x256" / "apps" / "sujian.png", 256)


def generate_packaging() -> None:
    copy_file(FULL_512, ROOT / "packaging" / "google-play" / "icon-512.png")
    copy_file(FULL_1024, ROOT / "packaging" / "apple" / "app-icon-1024.png")
    copy_file(FULL_512, ROOT / "packaging" / "web" / "icon-512.png")
    resize_png(FULL_1024, ROOT / "packaging" / "web" / "icon-192.png", 192)

    ico_path = ROOT / "packaging" / "windows" / "app.ico"
    ico_path.parent.mkdir(parents=True, exist_ok=True)
    with Image.open(FULL_1024) as image:
        image = image.convert("RGBA")
        image.save(
            ico_path,
            format="ICO",
            sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (256, 256)],
        )


def main() -> None:
    ensure_sources()
    generate_android()
    generate_linux()
    generate_packaging()


if __name__ == "__main__":
    main()
