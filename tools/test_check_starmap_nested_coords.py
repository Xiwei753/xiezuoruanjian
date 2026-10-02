#!/usr/bin/env python3
"""tools/test_check_starmap_nested_coords.py — 守卫脚本自测。

参考 tools/test_check_rust_safety_patterns.py 的 importlib 加载风格，
用 unittest 验证 check_starmap_nested_coords.check 的守卫逻辑正确。
"""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("check_starmap_nested_coords.py")
SPEC = importlib.util.spec_from_file_location("check_starmap_nested_coords", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


# 合法模板：满足守卫全部检查的三个文件内容。
CLEAN_NODE = """Item {
    function moveDelta(dx, dy) { root.moveDelta(dx, dy) }
}
"""

CLEAN_EMBED = """Item {
    function moveDelta(dx, dy) { root.moveDelta(dx, dy) }
}
"""

CLEAN_CANVAS = """Item {
    function sceneToCanvas(sx, sy) { return canvasArea.mapFromItem(null, sx, sy) }
    function sceneDeltaToWorld(dx, dy) { return { x: dx, y: dy } }
    onContextMenuRequested: function(sceneX, sceneY) {
        var cp = sceneToCanvas(sceneX, sceneY)
        var wp = sceneToWorld(sceneX, sceneY)
    }
}
"""


def _write_clean(tmp: Path) -> None:
    (tmp / "StarMapNode.qml").write_text(CLEAN_NODE, encoding="utf-8")
    (tmp / "StarMapEmbed.qml").write_text(CLEAN_EMBED, encoding="utf-8")
    (tmp / "StarMapCanvas.qml").write_text(CLEAN_CANVAS, encoding="utf-8")


class StarMapNestedCoordsGuardTests(unittest.TestCase):
    def _all_passed(self, results: list[tuple[bool, str]]) -> bool:
        return all(p for p, _ in results)

    def test_passes_on_clean_source(self) -> None:
        """对当前已修好的三个文件跑 check 应全 passed。"""
        qml_dir = Path(__file__).parent.parent / "apps" / "Linux_qt" / "qml"
        results = MODULE.check(qml_dir)
        # 打印便于诊断
        for passed, detail in results:
            self.assertTrue(passed, msg=f"clean source check failed: {detail}")

    def test_flags_canvasZoomLevel_in_node(self) -> None:
        """Node 含 canvasZoomLevel 应被标记失败。"""
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            _write_clean(tmp)
            (tmp / "StarMapNode.qml").write_text(
                "Item {\n    property real canvasZoomLevel: 1.0\n}\n",
                encoding="utf-8",
            )
            results = MODULE.check(tmp)
            self.assertFalse(self._all_passed(results), msg="canvasZoomLevel in Node not flagged")

    def test_flags_canvasZoomLevel_in_embed(self) -> None:
        """Embed 含 canvasZoomLevel 应被标记失败。"""
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            _write_clean(tmp)
            (tmp / "StarMapEmbed.qml").write_text(
                "Item {\n    property real canvasZoomLevel: 1.0\n}\n",
                encoding="utf-8",
            )
            results = MODULE.check(tmp)
            self.assertFalse(self._all_passed(results), msg="canvasZoomLevel in Embed not flagged")

    def test_flags_missing_mapFromItem_in_canvas(self) -> None:
        """Canvas 不含 mapFromItem(null 应被标记失败。"""
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            _write_clean(tmp)
            (tmp / "StarMapCanvas.qml").write_text(
                "Item {\n    function sceneDeltaToWorld(dx, dy) { return { x: dx, y: dy } }\n}\n",
                encoding="utf-8",
            )
            results = MODULE.check(tmp)
            self.assertFalse(self._all_passed(results), msg="missing mapFromItem(null) not flagged")

    def test_flags_missing_sceneDeltaToWorld_in_canvas(self) -> None:
        """Canvas 不含 sceneDeltaToWorld 应被标记失败。"""
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            _write_clean(tmp)
            (tmp / "StarMapCanvas.qml").write_text(
                "Item {\n    function sceneToCanvas(sx, sy) { return canvasArea.mapFromItem(null, sx, sy) }\n}\n",
                encoding="utf-8",
            )
            results = MODULE.check(tmp)
            self.assertFalse(self._all_passed(results), msg="missing sceneDeltaToWorld not flagged")

    def test_flags_screenToWorld_of_sceneX_in_canvas(self) -> None:
        """Canvas 含 screenToWorldX(sceneX) 应被标记失败。"""
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            _write_clean(tmp)
            (tmp / "StarMapCanvas.qml").write_text(
                "Item {\n"
                "    function sceneToCanvas(sx, sy) { return canvasArea.mapFromItem(null, sx, sy) }\n"
                "    function sceneDeltaToWorld(dx, dy) { return { x: dx, y: dy } }\n"
                "    onContextMenuRequested: function(sceneX, sceneY) {\n"
                "        var w = screenToWorldX(sceneX)\n"
                "    }\n"
                "}\n",
                encoding="utf-8",
            )
            results = MODULE.check(tmp)
            self.assertFalse(self._all_passed(results), msg="screenToWorldX(sceneX) not flagged")

    def test_flags_screenToWorld_of_sceneY_in_canvas(self) -> None:
        """Canvas 含 screenToWorldY(sceneY) 应被标记失败。"""
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            _write_clean(tmp)
            (tmp / "StarMapCanvas.qml").write_text(
                "Item {\n"
                "    function sceneToCanvas(sx, sy) { return canvasArea.mapFromItem(null, sx, sy) }\n"
                "    function sceneDeltaToWorld(dx, dy) { return { x: dx, y: dy } }\n"
                "    onContextMenuRequested: function(sceneX, sceneY) {\n"
                "        var w = screenToWorldY(sceneY)\n"
                "    }\n"
                "}\n",
                encoding="utf-8",
            )
            results = MODULE.check(tmp)
            self.assertFalse(self._all_passed(results), msg="screenToWorldY(sceneY) not flagged")


if __name__ == "__main__":
    unittest.main()
