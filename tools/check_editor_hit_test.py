#!/usr/bin/env python3
"""编辑器命中测试静态守卫。

验证 SujianEditor.onTouch 不用 fontSize*0.6 粗估，改用 EditorRenderBackend.hitTestByPoint
+ onAreaChange 实际容器宽度/高度 + getMeasureUtils 真实文本测量 + wordBreak(BREAK_ALL) 一致折行。
验证 EditorRenderBackend 含 hitTestByPoint 且 import editor_layout_math 纯数学模块。
验证 editor_layout_math.ts 存在且导出 layoutLines/hitTestPoint。
运行 editor_layout_math.test.mjs 纯逻辑单测（Node --experimental-strip-types）。

对应 Issue #629 第 5 节：命中测试归 EditorRenderBackend，SelectionController 接收
正确 UTF-16→UTF-8 offset。消除 eadae72c 的 fontSize*0.6 粗估与"后续改进"遗漏。
"""
import argparse
import os
import re
import subprocess
import sys
from pathlib import Path


def read_file(path: str) -> str | None:
    p = Path(path)
    if not p.is_file():
        return None
    return p.read_text(encoding="utf-8", errors="replace")


def strip_comments(src: str) -> str:
    """剥离 // 行注释与 /* */ 块注释，只留代码用于模式检查。"""
    src = re.sub(r"/\*.*?\*/", "", src, flags=re.DOTALL)
    src = re.sub(r"//.*$", "", src, flags=re.MULTILINE)
    return src


def main() -> None:
    parser = argparse.ArgumentParser(description="编辑器命中测试静态守卫")
    parser.add_argument("--harmony-root", default="apps/harmony")
    args = parser.parse_args()
    harmony_root = os.path.normpath(args.harmony_root)
    ets_root = os.path.join(harmony_root, "entry/src/main/ets")

    results: list[tuple[bool, str]] = []

    # ── 检查 1: SujianEditor.ets 命中测试实现 ──
    sujian_path = os.path.join(ets_root, "feature/editor/ui/SujianEditor.ets")
    sujian = read_file(sujian_path)
    if sujian is None:
        results.append((False, f"SujianEditor.ets 不存在: {sujian_path}"))
    else:
        code = strip_comments(sujian)
        # 1a: 代码不含 *0.6 粗估
        has_06 = bool(re.search(r"\*\s*0\.6", code))
        results.append((
            not has_06,
            f"SujianEditor.ets 代码不含 *0.6 粗估 — {'不含' if not has_06 else '发现 *0.6'}",
        ))
        # 1b: 代码不含 TODO/占位/fallback
        bad_words = [w for w in ("TODO", "占位", "fallback") if w in code]
        results.append((
            len(bad_words) == 0,
            f"SujianEditor.ets 代码不含 TODO/占位/fallback — {'不含' if not bad_words else f'发现 {bad_words}'}",
        ))
        # 1c: 含 onAreaChange（实际容器宽度/高度）
        results.append(("onAreaChange" in code, f"SujianEditor.ets 含 onAreaChange — {'含' if 'onAreaChange' in code else '缺失'}"))
        # 1d: 调 EditorRenderBackend.hitTestByPoint
        results.append(("EditorRenderBackend.hitTestByPoint" in code, f"SujianEditor.ets 调 EditorRenderBackend.hitTestByPoint — {'调' if 'EditorRenderBackend.hitTestByPoint' in code else '缺失'}"))
        # 1e: 含 getMeasureUtils（真实文本测量）
        results.append(("getMeasureUtils" in code, f"SujianEditor.ets 含 getMeasureUtils 真实测量 — {'含' if 'getMeasureUtils' in code else '缺失'}"))
        # 1f: 含 wordBreak（一致折行）
        results.append(("wordBreak" in code, f"SujianEditor.ets 含 wordBreak — {'含' if 'wordBreak' in code else '缺失'}"))

    # ── 检查 2: EditorRenderBackend.ets 含 hitTestByPoint 且 import editor_layout_math ──
    backend_path = os.path.join(ets_root, "feature/editor/render/EditorRenderBackend.ets")
    backend = read_file(backend_path)
    if backend is None:
        results.append((False, f"EditorRenderBackend.ets 不存在: {backend_path}"))
    else:
        results.append(("hitTestByPoint" in backend, f"EditorRenderBackend.ets 含 hitTestByPoint — {'含' if 'hitTestByPoint' in backend else '缺失'}"))
        results.append(("editor_layout_math" in backend, f"EditorRenderBackend.ets import editor_layout_math — {'含' if 'editor_layout_math' in backend else '缺失'}"))

    # ── 检查 3: editor_layout_math.ts 存在且导出 layoutLines/hitTestPoint ──
    math_path = os.path.join(ets_root, "feature/editor/render/editor_layout_math.ts")
    math_src = read_file(math_path)
    if math_src is None:
        results.append((False, f"editor_layout_math.ts 不存在: {math_path}"))
    else:
        has_layout = "export function layoutLines" in math_src
        has_hit = "export function hitTestPoint" in math_src
        results.append((
            has_layout and has_hit,
            f"editor_layout_math.ts 导出 layoutLines/hitTestPoint — {'含' if has_layout and has_hit else '缺失'}",
        ))

    # ── 检查 4: 运行 Node 纯逻辑单测 ──
    test_path = os.path.join(ets_root, "feature/editor/render/__tests__/editor_layout_math.test.mjs")
    if not os.path.isfile(test_path):
        results.append((False, f"单测文件不存在: {test_path}"))
    else:
        try:
            proc = subprocess.run(
                ["node", "--experimental-strip-types", test_path],
                capture_output=True, text=True, timeout=30,
            )
            ok = proc.returncode == 0 and "tests passed" in proc.stdout
            out_last = proc.stdout.strip().splitlines()[-1] if proc.stdout.strip() else ""
            detail = f"exit={proc.returncode}, {out_last}"
            if not ok and proc.stderr:
                err_last = proc.stderr.strip().splitlines()[-1]
                detail += f", stderr={err_last}"
            results.append((ok, f"Node 纯逻辑单测 — {detail}"))
        except Exception as e:
            results.append((False, f"Node 单测异常: {e}"))

    # ── 输出 ──
    print("=" * 60)
    print("编辑器命中测试静态守卫")
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
