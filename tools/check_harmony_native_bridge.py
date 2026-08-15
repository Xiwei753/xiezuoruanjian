#!/usr/bin/env python3
"""
Harmony Native Bridge 一致性静态守卫脚本。

检查 Harmony native 桥接的完整性，确保不会落回 mock、
NAPI 注册与 ArkTS 调用一致、C header 与 Rust FFI 导出一致等。
"""

import argparse
import os
import re
import sys
from pathlib import Path
from typing import List, Tuple


# ---------------------------------------------------------------------------
# 工具函数
# ---------------------------------------------------------------------------

def read_file(path: str) -> str:
    """读取文件内容，不存在则返回 None。"""
    p = Path(path)
    if not p.is_file():
        return None
    return p.read_text(encoding="utf-8", errors="replace")


def result_line(name: str, passed: bool, detail: str = "") -> str:
    status = "PASS" if passed else "FAIL"
    line = f"  [{status}] {name}"
    if detail:
        line += f" — {detail}"
    return line


# ---------------------------------------------------------------------------
# 检查 1: NativeWriterCoreBridge.ets 不能落回 mock
# ---------------------------------------------------------------------------

def check_native_bridge_no_mock(harmony_root: str) -> List[Tuple[bool, str]]:
    results: List[Tuple[bool, str]] = []

    # 1a: 扫描所有 Native*Bridge.ets 文件，检查不能有 MockWriterCoreBridge 的 import 或使用
    # #629 迁移后：NativeWriterCoreBridge 在 corebridge/ 根，其余 Native*Bridge 与
    # NativeCoreModule 在 corebridge/native/。NativeWorkspaceBridge 已随 Workspace
    # 拆成 Project 而移除。
    bridge_files = [
        "NativeWriterCoreBridge.ets",
        "NativeCoreModule.ets",
        "NativeProjectBridge.ets",
        "NativeChapterBridge.ets",
        "NativeSettingsBridge.ets",
        "NativeSyncBridge.ets",
        "NativeStatsBridge.ets",
        "NativeStarMapBridge.ets",
        "NativeLayoutBridge.ets",
    ]
    ets_corebridge_root = "entry/src/main/ets/corebridge"
    ets_corebridge_native = "entry/src/main/ets/corebridge/native"
    all_bridge_content = ""
    for bf in bridge_files:
        if bf == "NativeWriterCoreBridge.ets":
            fpath = os.path.join(harmony_root, ets_corebridge_root, bf)
        else:
            fpath = os.path.join(harmony_root, ets_corebridge_native, bf)
        c = read_file(fpath)
        if c is not None:
            all_bridge_content += c + "\n"
    if not all_bridge_content.strip():
        results.append((False, "No Native*Bridge.ets files found"))
        return results

    has_mock_import = bool(re.search(r"import\s+.*MockWriterCoreBridge", all_bridge_content))
    has_mock_usage = bool(re.search(r"\bMockWriterCoreBridge\b", all_bridge_content))
    mock_ok = not has_mock_import and not has_mock_usage
    detail = ""
    if has_mock_import:
        detail += "发现 MockWriterCoreBridge import; "
    if has_mock_usage:
        detail += "发现 MockWriterCoreBridge 使用; "
    results.append((mock_ok, f"Native*Bridge.ets 不含 MockWriterCoreBridge — {detail.strip(' ;')}"))

    # 1b: initialize() 失败时不能降级到 mock
    # 只检查 NativeWriterCoreBridge.ets 中的 initialize 方法
    main_bridge_path = os.path.join(harmony_root, "entry/src/main/ets/corebridge/NativeWriterCoreBridge.ets")
    main_bridge_content = read_file(main_bridge_path)
    if main_bridge_content is None:
        results.append((False, f"文件不存在: {main_bridge_path}"))
        return results

    init_match = re.search(
        r"(?:async\s+)?initialize\s*\([^)]*\)\s*(?::\s*[^{]+?)?\{",
        main_bridge_content,
    )
    degrade_ok = True
    degrade_detail = ""
    if init_match:
        start = init_match.end() - 1
        depth = 0
        end = start
        for i in range(start, len(main_bridge_content)):
            if main_bridge_content[i] == "{":
                depth += 1
            elif main_bridge_content[i] == "}":
                depth -= 1
                if depth == 0:
                    end = i + 1
                    break
        init_body = main_bridge_content[start:end]

        init_body_no_comments = re.sub(r"//.*$", "", init_body, flags=re.MULTILINE)
        init_body_no_comments = re.sub(r"/\*.*?\*/", "", init_body_no_comments, flags=re.DOTALL)

        if re.search(r"[Mm]ock", init_body_no_comments):
            degrade_ok = False
            degrade_detail = "initialize() 方法体中包含 mock 降级逻辑"
        if re.search(r"fallback.*mock|mock.*fallback", init_body_no_comments, re.IGNORECASE):
            degrade_ok = False
            degrade_detail = "initialize() 方法体中包含 fallback 到 mock 的逻辑"
        if degrade_ok and not degrade_detail:
            degrade_detail = "initialize() 不含 mock 降级"
    else:
        degrade_ok = False
        degrade_detail = "未找到 initialize() 方法定义"

    results.append((degrade_ok, f"NativeWriterCoreBridge.ets initialize() 不降级到 mock — {degrade_detail}"))

    return results


# ---------------------------------------------------------------------------
# 检查 2: AppContext 装配 NativeWriterCoreBridge 且无 mock 切换
# ---------------------------------------------------------------------------

def check_app_context_native_default(harmony_root: str) -> List[Tuple[bool, str]]:
    results: List[Tuple[bool, str]] = []
    # #629 迁移后：AppContext 从 common/ 移到 app/，不再有 environment:'native' 字段，
    # 而是在构造时直接 this.bridge = NativeWriterCoreBridge.getInstance()。
    fpath = os.path.join(harmony_root, "entry/src/main/ets/app/AppContext.ets")
    content = read_file(fpath)

    if content is None:
        results.append((False, f"文件不存在: {fpath}"))
        return results

    # 2a: 必须导入 NativeWriterCoreBridge 并在构造中装配 getInstance()
    has_import = bool(re.search(r"import\s+.*NativeWriterCoreBridge", content))
    has_assemble = bool(re.search(r"NativeWriterCoreBridge\s*\.\s*getInstance\s*\(\s*\)", content))
    assemble_ok = has_import and has_assemble
    detail_parts = []
    if not has_import:
        detail_parts.append("未找到 NativeWriterCoreBridge import")
    if not has_assemble:
        detail_parts.append("未找到 NativeWriterCoreBridge.getInstance() 装配")
    detail = "; ".join(detail_parts) if detail_parts else "import 与 getInstance() 装配均存在"
    results.append((assemble_ok, f"AppContext.ets 装配 NativeWriterCoreBridge — {detail}"))

    # 2b: 不能含 mock/native 运行时切换或 mock 桥接
    has_mock_ref = bool(re.search(r"\bMockWriterCoreBridge\b", content))
    has_smoke_test = bool(re.search(r"nativeSmokeTest", content))
    has_env_mock = bool(re.search(r"environment\s*===?\s*['\"]mock['\"]", content))
    has_use_mock = bool(re.search(r"\buseMock\b", content))
    no_switch_ok = not (has_mock_ref or has_smoke_test or has_env_mock or has_use_mock)
    bad = []
    if has_mock_ref:
        bad.append("MockWriterCoreBridge")
    if has_smoke_test:
        bad.append("nativeSmokeTest")
    if has_env_mock:
        bad.append("environment === 'mock'")
    if has_use_mock:
        bad.append("useMock")
    detail = "无 mock 切换" if no_switch_ok else f"发现禁止项: {bad}"
    results.append((no_switch_ok, f"AppContext.ets 不含 mock/native 运行时切换 — {detail}"))

    return results


# ---------------------------------------------------------------------------
# 检查 3: EntryAbility 必须 setAbilityContext
# ---------------------------------------------------------------------------

def check_entry_ability_set_context(harmony_root: str) -> List[Tuple[bool, str]]:
    results: List[Tuple[bool, str]] = []
    fpath = os.path.join(harmony_root, "entry/src/main/ets/entryability/EntryAbility.ets")
    content = read_file(fpath)

    if content is None:
        results.append((False, f"文件不存在: {fpath}"))
        return results

    on_create_match = re.search(
        r"(?:async\s+)?onCreate\s*\([^)]*\)\s*(?::\s*[^{]+?)?\{",
        content,
    )
    if on_create_match is None:
        results.append((False, "EntryAbility.ets 未找到 onCreate() 方法"))
        return results

    start = on_create_match.end() - 1
    depth = 0
    end = start
    for i in range(start, len(content)):
        if content[i] == "{":
            depth += 1
        elif content[i] == "}":
            depth -= 1
            if depth == 0:
                end = i + 1
                break
    body = content[start:end]

    has_set = bool(re.search(r"getAppContext\(\)\s*\.?\s*setAbilityContext\s*\(", body))
    results.append((
        has_set,
        f"EntryAbility.ets onCreate() 调用 getAppContext().setAbilityContext() — {'存在' if has_set else '未找到'}",
    ))

    return results


# ---------------------------------------------------------------------------
# 检查 4: Index 只做 Navigation 宿主
# ---------------------------------------------------------------------------

def check_index_native_init(harmony_root: str) -> List[Tuple[bool, str]]:
    results: List[Tuple[bool, str]] = []
    # #629 后 Index.ets 简化为只渲染 AppNavigation，不再有 initAndLoad/nativeBridge。
    # 初始化在 AppContext 构造 + EntryAbility.onCreate。
    fpath = os.path.join(harmony_root, "entry/src/main/ets/pages/Index.ets")
    content = read_file(fpath)

    if content is None:
        results.append((False, f"文件不存在: {fpath}"))
        return results

    # 4a: 必须导入 AppNavigation 并在 build() 中渲染 AppNavigation()
    has_nav_import = bool(re.search(r"import\s+.*AppNavigation", content))
    has_nav_render = bool(re.search(r"AppNavigation\s*\(\s*\)", content))
    nav_ok = has_nav_import and has_nav_render
    detail_parts = []
    if not has_nav_import:
        detail_parts.append("未找到 AppNavigation import")
    if not has_nav_render:
        detail_parts.append("未找到 AppNavigation() 渲染")
    detail = "; ".join(detail_parts) if detail_parts else "import 与渲染均存在"
    results.append((nav_ok, f"Index.ets 渲染 AppNavigation — {detail}"))

    # 4b: 不应堆业务逻辑：不含 initAndLoad/nativeBridge/MockWriterCoreBridge/mock 降级
    has_init_and_load = bool(re.search(r"\binitAndLoad\b", content))
    has_native_bridge = bool(re.search(r"\bnativeBridge\b", content))
    has_mock_ref = bool(re.search(r"\bMockWriterCoreBridge\b", content))
    has_mock_degrade = bool(re.search(r"[Mm]ock", content))
    clean_ok = not (has_init_and_load or has_native_bridge or has_mock_ref or has_mock_degrade)
    bad = []
    if has_init_and_load:
        bad.append("initAndLoad")
    if has_native_bridge:
        bad.append("nativeBridge")
    if has_mock_ref:
        bad.append("MockWriterCoreBridge")
    if has_mock_degrade:
        bad.append("mock")
    detail = "无业务逻辑残留" if clean_ok else f"发现禁止项: {bad}"
    results.append((clean_ok, f"Index.ets 不堆业务逻辑 — {detail}"))

    return results


# ---------------------------------------------------------------------------
# 检查 5: CMakeLists.txt 必须检查 prebuilt SO
# ---------------------------------------------------------------------------

def check_cmake_prebuilt_so(harmony_root: str) -> List[Tuple[bool, str]]:
    results: List[Tuple[bool, str]] = []
    fpath = os.path.join(harmony_root, "entry/src/main/cpp/CMakeLists.txt")
    content = read_file(fpath)

    if content is None:
        results.append((False, f"文件不存在: {fpath}"))
        return results

    has_fatal = bool(re.search(r"FATAL_ERROR", content))
    has_so_check = bool(re.search(r"libwriter_core_ffi\.so", content))

    ok = has_fatal and has_so_check
    detail_parts = []
    if not has_fatal:
        detail_parts.append("未找到 FATAL_ERROR")
    if not has_so_check:
        detail_parts.append("未找到 libwriter_core_ffi.so 检查")
    detail = "; ".join(detail_parts) if detail_parts else "存在 FATAL_ERROR 和 libwriter_core_ffi.so 检查"

    results.append((ok, f"CMakeLists.txt 包含 FATAL_ERROR + libwriter_core_ffi.so 检查 — {detail}"))

    return results


# ---------------------------------------------------------------------------
# 检查 6: napi_init.cpp 注册函数名 vs NativeWriterCoreBridge.ets 调用名
# ---------------------------------------------------------------------------

def check_napi_vs_arkts(harmony_root: str) -> List[Tuple[bool, str]]:
    results: List[Tuple[bool, str]] = []

    # 6a: 提取所有 napi_*.cpp 中注册的函数名
    # #629 迁移后：napi_*.cpp 从 cpp/ 移到 cpp/corebridge/napi/，并新增 napi_editor_session.cpp。
    napi_files = [
        "napi_init.cpp",
        "napi_app_state.cpp",
        "napi_project.cpp",
        "napi_chapter.cpp",
        "napi_settings.cpp",
        "napi_sync.cpp",
        "napi_stats.cpp",
        "napi_starmap.cpp",
        "napi_editor_session.cpp",
    ]
    cpp_napi_dir = "entry/src/main/cpp/corebridge/napi"
    napi_content = ""
    for nf in napi_files:
        fpath = os.path.join(harmony_root, cpp_napi_dir, nf)
        content = read_file(fpath)
        if content is not None:
            napi_content += content + "\n"
    if not napi_content.strip():
        results.append((False, "No napi_*.cpp files found"))
        return results

    napi_funcs = set()
    for m in re.finditer(r"\"(native\w+)\"", napi_content):
        napi_funcs.add(m.group(1))

    for m in re.finditer(r"napi_set_property_name\s*\([^,]+,\s*\"(\w+)\"", napi_content):
        napi_funcs.add(m.group(1))

    # 6b: 提取所有 Native*Bridge.ets 中调用的函数名
    # #629 迁移后：NativeWriterCoreBridge 在 corebridge/ 根，其余 Native*Bridge 与
    # NativeCoreModule 在 corebridge/native/，NativeEditorSessionBridge 在
    # feature/editor/interop/。NativeWorkspaceBridge 已移除。
    bridge_locations = [
        ("NativeWriterCoreBridge.ets", "entry/src/main/ets/corebridge"),
        ("NativeCoreModule.ets", "entry/src/main/ets/corebridge/native"),
        ("NativeProjectBridge.ets", "entry/src/main/ets/corebridge/native"),
        ("NativeChapterBridge.ets", "entry/src/main/ets/corebridge/native"),
        ("NativeSettingsBridge.ets", "entry/src/main/ets/corebridge/native"),
        ("NativeSyncBridge.ets", "entry/src/main/ets/corebridge/native"),
        ("NativeStatsBridge.ets", "entry/src/main/ets/corebridge/native"),
        ("NativeStarMapBridge.ets", "entry/src/main/ets/corebridge/native"),
        ("NativeLayoutBridge.ets", "entry/src/main/ets/corebridge/native"),
        ("NativeEditorSessionBridge.ets", "entry/src/main/ets/feature/editor/interop"),
    ]
    bridge_content = ""
    for bf, subdir in bridge_locations:
        fpath = os.path.join(harmony_root, subdir, bf)
        c = read_file(fpath)
        if c is not None:
            bridge_content += c + "\n"
    if not bridge_content.strip():
        results.append((False, "No Native*Bridge.ets files found"))
        return results

    arkts_funcs = set()
    for m in re.finditer(
        r"(?:getNativeModule|nativeModule)\s*\(\s*\)\s*\.\s*(native\w+)\s*\(",
        bridge_content,
    ):
        arkts_funcs.add(m.group(1))

    for m in re.finditer(r"this\.nativeModule\.\s*(native\w+)\s*\(", bridge_content):
        arkts_funcs.add(m.group(1))

    # 6c: 比对
    missing_in_napi = arkts_funcs - napi_funcs
    ok = len(missing_in_napi) == 0
    if ok:
        detail = f"NAPI 注册 {len(napi_funcs)} 个函数, ArkTS 调用 {len(arkts_funcs)} 个函数, 全部覆盖"
    else:
        detail = f"NAPI 注册 {len(napi_funcs)} 个函数, ArkTS 调用 {len(arkts_funcs)} 个函数; NAPI 缺失: {sorted(missing_in_napi)}"

    results.append((ok, f"napi_init.cpp 注册函数覆盖 ArkTS 调用 — {detail}"))

    return results


# ---------------------------------------------------------------------------
# 检查 7: writer_core_bridge.h 声明 vs Rust ffi/mod.rs 导出
# ---------------------------------------------------------------------------

def check_c_header_vs_rust_ffi(harmony_root: str, core_root: str) -> List[Tuple[bool, str]]:
    results: List[Tuple[bool, str]] = []

    # 7a: 提取 writer_core_bridge.h 中的函数声明
    # #629 迁移后：header 从 cpp/ 移到 cpp/corebridge/writer_core_bridge.h。
    header_path = os.path.join(harmony_root, "entry/src/main/cpp/corebridge/writer_core_bridge.h")
    header_content = read_file(header_path)
    if header_content is None:
        results.append((False, f"文件不存在: {header_path}"))
        return results

    header_funcs = set()
    for m in re.finditer(r"\b(writer_core_\w+)\s*\(", header_content):
        header_funcs.add(m.group(1))

    # 7b: 提取 Rust ffi 目录下所有 .rs 文件中的导出函数
    ffi_dir = os.path.join(core_root, "src/ffi")
    rust_funcs = set()
    if os.path.isdir(ffi_dir):
        for rs_file in Path(ffi_dir).rglob("*.rs"):
            rs_content = rs_file.read_text(encoding="utf-8", errors="replace")
            for m in re.finditer(
                r"#\[no_mangle\]\s*(?:(?:#\[[^\]]*\]|//[^\n]*\n\s*)\s*)*pub\s+unsafe\s+extern\s+\"C\"\s+fn\s+(writer_core_\w+)",
                rs_content,
            ):
                rust_funcs.add(m.group(1))
    else:
        results.append((False, f"Rust ffi 目录不存在: {ffi_dir}"))
        return results

    # 7c: 比对
    in_header_not_in_rust = header_funcs - rust_funcs
    in_rust_not_in_header = rust_funcs - header_funcs

    ok = len(in_header_not_in_rust) == 0
    if ok:
        detail = f"C header 声明 {len(header_funcs)} 个函数, Rust FFI 导出 {len(rust_funcs)} 个函数, C header 是 Rust 子集"
    else:
        detail = f"C header 有但 Rust 缺失: {sorted(in_header_not_in_rust)}"

    results.append((ok, f"writer_core_bridge.h 声明是 Rust FFI 导出的子集 — {detail}"))

    if in_rust_not_in_header:
        results.append((
            True,
            f"Rust FFI 有但 C header 缺失（可能 Desktop 专用）: {sorted(in_rust_not_in_header)}",
        ))

    return results


# ---------------------------------------------------------------------------
# 检查 8: README 不能写未接入 Rust Core / mock-only
# ---------------------------------------------------------------------------

def check_readme_no_mock_claims(harmony_root: str) -> List[Tuple[bool, str]]:
    results: List[Tuple[bool, str]] = []
    fpath = os.path.join(harmony_root, "README.md")
    content = read_file(fpath)

    if content is None:
        results.append((True, "README.md 不存在，跳过检查"))
        return results

    forbidden_patterns = [
        (r"未接入\s*Rust\s*Core", "未接入 Rust Core"),
        (r"mock-only", "mock-only"),
        (r"Native\s*空壳[，,]?\s*待实现", "Native 空壳，待实现"),
        (r"真实桥接待开始", "真实桥接待开始"),
        (r"当前所有页面和桥接均使用\s*mock\s*数据", "当前所有页面和桥接均使用 mock 数据"),
    ]

    found = []
    for pattern, label in forbidden_patterns:
        if re.search(pattern, content):
            found.append(label)

    ok = len(found) == 0
    detail = "无禁止用语" if ok else f"发现禁止用语: {found}"
    results.append((ok, f"README.md 不含 mock/未接入 声明 — {detail}"))

    return results


# ---------------------------------------------------------------------------
# 主函数
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="Harmony Native Bridge 一致性静态守卫"
    )
    parser.add_argument(
        "--harmony-root",
        default="apps/harmony",
        help="Harmony 应用根目录（默认: apps/harmony）",
    )
    parser.add_argument(
        "--core-root",
        default="core/writer_core",
        help="Rust writer_core 根目录（默认: core/writer_core）",
    )
    args = parser.parse_args()

    harmony_root = os.path.normpath(args.harmony_root)
    core_root = os.path.normpath(args.core_root)

    print("=" * 60)
    print("Harmony Native Bridge 一致性检查")
    print("=" * 60)
    print(f"  harmony-root: {harmony_root}")
    print(f"  core-root:    {core_root}")
    print()

    all_results: List[Tuple[bool, str]] = []

    checks = [
        ("1. NativeWriterCoreBridge.ets 不落回 mock", check_native_bridge_no_mock, harmony_root),
        ("2. AppContext 装配 NativeWriterCoreBridge 且无 mock 切换", check_app_context_native_default, harmony_root),
        ("3. EntryAbility 必须 setAbilityContext", check_entry_ability_set_context, harmony_root),
        ("4. Index 只做 Navigation 宿主", check_index_native_init, harmony_root),
        ("5. CMakeLists.txt 必须检查 prebuilt SO", check_cmake_prebuilt_so, harmony_root),
        ("6. NAPI 注册 vs ArkTS 调用", check_napi_vs_arkts, harmony_root),
        ("7. C header vs Rust FFI", lambda hr: check_c_header_vs_rust_ffi(hr, core_root), harmony_root),
        ("8. README 不含 mock 声明", check_readme_no_mock_claims, harmony_root),
    ]

    for title, check_fn, root in checks:
        print(f"[检查] {title}")
        try:
            results = check_fn(root)
        except Exception as e:
            results = [(False, f"检查异常: {e}")]
        for passed, detail in results:
            print(result_line("", passed, detail))
            all_results.append((passed, detail))
        print()

    failures = sum(1 for passed, _ in all_results if not passed)
    print("=" * 60)
    if failures == 0:
        print("ALL PASS")
    else:
        print(f"{failures} FAILURES")
    print("=" * 60)

    sys.exit(1 if failures > 0 else 0)


if __name__ == "__main__":
    main()
