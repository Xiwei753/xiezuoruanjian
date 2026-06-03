# 素笺写作应用图标

`source/` 是应用图标源文件的唯一源头。Android、Linux、Windows、Google Play、Apple 和 Web/README 预览资源都必须从这里派生。

禁止各平台自行重绘、改色或改风格。需要更新应用图标时，先替换 `source/` 中的 v10 源文件，再运行 `scripts/generate_icons.py` 重新生成平台资源。

当前源文件：

- `source/sujian_icon.svg`: 完整应用图标 SVG。
- `source/sujian_icon_foreground.svg`: Android adaptive icon 前景层 SVG 源文件。
- `source/sujian_icon_background_white.svg`: Android adaptive icon 白色背景层 SVG 源文件。
- `source/sujian_icon_1024.png`: 1024x1024 完整应用图标。
- `source/sujian_icon_512.png`: 512x512 完整应用图标。
- `source/sujian_icon_foreground_1024.png`: Android adaptive icon 前景层 PNG 源文件。

生成依赖：

- Python 3
- Pillow

生成命令：

```bash
python3 scripts/generate_icons.py
```
