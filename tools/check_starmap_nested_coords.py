#!/usr/bin/env python3
"""星图递归子星图坐标统一映射静态守卫。

Issue #814 评论 5947740838: 修复 Linux_Qt 星图递归子星图坐标漂移。
根因：Qt DragHandler(target:null) 的 activeTranslation 是 scene 坐标增量，
不会 mapFromScene 到 handler parent。旧实现 Node/Embed 的 5 个 DragHandler
只除以本层 zoomLevel，漏掉父 Embed、祖父 Embed 的 scale，嵌套 Scene 拖动按
祖先缩放比例漂移；右键菜单上抛的 eventPoint.scenePosition 也被直接喂给
screenToWorld*()（实际接收 Canvas local 坐标），递归下同样偏。

修复方案：统一在 StarMapCanvas.qml 用 Item.mapFromItem(null,...) 做
scene→本 Canvas local→world 映射；Node/Embed 只上抛 raw scene delta，
不再自己除 zoom，并删除 canvasZoomLevel 属性。

Issue #832 评论 5998365709 后续：输入收成单一 StarMapInputRouter，Node/Embed
不再各自上抛 scene delta，scene→world 映射不再有调用方；Canvas 只保留
sceneDeltaToCanvas 作为 Router pan 的统一 scene→canvas 入口。守卫随之从
sceneDeltaToWorld 改为锁定 sceneDeltaToCanvas。

本守卫锁定三件事，防止回归：
  1. StarMapNode.qml / StarMapEmbed.qml 不含 canvasZoomLevel（属性已删）。
  2. StarMapCanvas.qml 含 mapFromItem(null 的 scene→canvas 映射，且含
     sceneDeltaToCanvas 统一入口。
  3. StarMapCanvas.qml 的 onContextMenuRequested 处不能出现
     screenToWorldX(sceneX) / screenToWorldY(sceneY)（菜单 scene 坐标不能
     直接喂 screenToWorld*，必须先经 sceneToCanvas/sceneToWorld 映射）。
"""

from __future__ import annotations

import sys
from pathlib import Path


def read_file(path: Path) -> str | None:
    """读取文件内容，文件不存在返回 None。"""
    if not path.is_file():
        return None
    return path.read_text(encoding="utf-8", errors="replace")


def check(qml_dir: Path) -> list[tuple[bool, str]]:
    """对 qml_dir 下三个星图 QML 文件执行守卫检查，返回 (passed, detail) 列表。"""
    results: list[tuple[bool, str]] = []

    node_path = qml_dir / "StarMapNode.qml"
    embed_path = qml_dir / "StarMapEmbed.qml"
    canvas_path = qml_dir / "StarMapCanvas.qml"

    node = read_file(node_path)
    embed = read_file(embed_path)
    canvas = read_file(canvas_path)

    # ── 检查 1a: StarMapNode.qml 不含 canvasZoomLevel ──
    if node is None:
        results.append((False, f"StarMapNode.qml 不存在: {node_path}"))
    else:
        has = "canvasZoomLevel" in node
        results.append((
            not has,
            f"StarMapNode.qml 不含 canvasZoomLevel — {'不含' if not has else '发现 canvasZoomLevel'}",
        ))

    # ── 检查 1b: StarMapEmbed.qml 不含 canvasZoomLevel ──
    if embed is None:
        results.append((False, f"StarMapEmbed.qml 不存在: {embed_path}"))
    else:
        has = "canvasZoomLevel" in embed
        results.append((
            not has,
            f"StarMapEmbed.qml 不含 canvasZoomLevel — {'不含' if not has else '发现 canvasZoomLevel'}",
        ))

    # ── 检查 2: StarMapCanvas.qml 含 mapFromItem(null 且含 sceneDeltaToCanvas ──
    if canvas is None:
        results.append((False, f"StarMapCanvas.qml 不存在: {canvas_path}"))
        results.append((False, "StarMapCanvas.qml 含 mapFromItem(null + sceneDeltaToCanvas — 文件缺失"))
        results.append((False, "StarMapCanvas.qml 不含 screenToWorldX(sceneX)/screenToWorldY(sceneY) — 文件缺失"))
    else:
        has_mapfrom = "mapFromItem(null" in canvas
        has_helper = "sceneDeltaToCanvas" in canvas
        results.append((
            has_mapfrom and has_helper,
            f"StarMapCanvas.qml 含 mapFromItem(null + sceneDeltaToCanvas 统一映射 — "
            f"{'含' if has_mapfrom and has_helper else f'mapFromItem(null={has_mapfrom}, sceneDeltaToCanvas={has_helper}'}",
        ))

        # ── 检查 3: Canvas 不含 screenToWorldX(sceneX) / screenToWorldY(sceneY) ──
        bad_x = "screenToWorldX(sceneX)" in canvas
        bad_y = "screenToWorldY(sceneY)" in canvas
        results.append((
            not bad_x and not bad_y,
            f"StarMapCanvas.qml 不含 screenToWorldX(sceneX)/screenToWorldY(sceneY) — "
            f"{'不含' if not bad_x and not bad_y else f'发现 screenToWorldX(sceneX)={bad_x}, screenToWorldY(sceneY)={bad_y}'}",
        ))

    return results


def main() -> None:
    qml_dir = Path(__file__).parent.parent / "apps" / "Linux_qt" / "qml"
    if not qml_dir.is_dir():
        print(f"QML directory not found: {qml_dir}")
        sys.exit(1)

    results = check(qml_dir)

    print("=" * 60)
    print("星图递归子星图坐标统一映射守卫 (Issue #814 评论 5947740838)")
    print("=" * 60)
    failures = 0
    for passed, detail in results:
        status = "PASS" if passed else "FAIL"
        print(f"  [{status}] {detail}")
        if not passed:
            failures += 1
    print("=" * 60)
    print("ALL PASS" if failures == 0 else f"{failures} FAILURES")
    print("=" * 60)
    sys.exit(1 if failures > 0 else 0)


if __name__ == "__main__":
    main()
