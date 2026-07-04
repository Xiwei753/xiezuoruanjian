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
    fpath = os.path.join(harmony_root, "entry/src/main/ets/bridge/NativeWriterCoreBridge.ets")
    content = read_file(fpath)

    if content is None:
        results.append((False, f"文件不存在: {fpath}"))
        return results

    # 1a: 不能有 MockWriterCoreBridge 的 import 或使用
    has_mock_import = bool(re.search(r"import\s+.*MockWriterCoreBridge", content))
    has_mock_usage = bool(re.search(r"\bMockWriterCoreBridge\b", content))
    mock_ok = not has_mock_import and not has_mock_usage
    detail = ""
    if has_mock_import:
        detail += "发现 MockWriterCoreBridge import; "
    if has_mock_usage:
        detail += "发现 MockWriterCoreBridge 使用; "
    results.append((mock_ok, f"NativeWriterCoreBridge.ets 不含 MockWriterCoreBridge — {detail.strip(' ;')}"))

    # 1b: initialize() 失败时不能降级到 mock
    # 查找 initialize 方法体，检查是否有 fallback 到 mock 的逻辑
    init_match = re.search(
        r"(?:async\s+)?initialize\s*\([^)]*\)\s*(?::\s*[^{]+?)?\{",
        content,
    )
    degrade_ok = True
    degrade_detail = ""
    if init_match:
        # 从匹配位置开始，找到对应的大括号闭合
        start = init_match.end() - 1  # 指向 {
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
        init_body = content[start:end]

        # 去除注释行再检查，避免误报
        init_body_no_comments = re.sub(r"//.*$", "", init_body, flags=re.MULTILINE)
        init_body_no_comments = re.sub(r"/\*.*?\*/", "", init_body_no_comments, flags=re.DOTALL)

        # 检查是否降级到 mock
        if re.search(r"[Mm]ock", init_body_no_comments):
            degrade_ok = False
            degrade_detail = "initialize() 方法体中包含 mock 降级逻辑"
        if re.search(r"fallback.*mock|mock.*fallback", init_body_no_comments, re.IGNORECASE):
            degrade_ok = False
            degrade_detail = "initialize() 方法体中包含 fallback 到 mock 的逻辑"
    else:
        degrade_ok = False
        degrade_detail = "未找到 initialize() 方法定义"

    results.append((degrade_ok, f"NativeWriterCoreBridge.ets initialize() 不降级到 mock — {degrade_detail}"))

    return results


# ---------------------------------------------------------------------------
# 检查 2: AppContext 默认 native
# ---------------------------------------------------------------------------

def check_app_context_native_default(harmony_root: str) -> List[Tuple[bool, str]]:
    results: List[Tuple[bool, str]] = []
    fpath = os.path.join(harmony_root, "entry/src/main/ets/common/AppContext.ets")
    content = read_file(fpath)

    if content is None:
        results.append((False, f"文件不存在: {fpath}"))
        return results

    # 2a: 默认 environment 必须是 'native'
    has_native_default = bool(re.search(r"environment\s*:\s*['\"]native['\"]", content))
    native_detail = "存在" if has_native_default else "未找到 environment: 'native' 默认值"
    results.append((
        has_native_default,
        f"AppContext.ets 默认 environment='native' — {native_detail}",
    ))

    # 2b: markNativeInitFailed() 中不能有降级到 mock 的逻辑
    mark_match = re.search(
        r"(?:async\s+)?markNativeInitFailed\s*\([^)]*\)\s*(?::\s*[^{]+?)?\{",
        content,
    )
    degrade_ok = True
    degrade_detail = "无降级逻辑"
    if mark_match:
        start = mark_match.end() - 1
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
        # 去除注释行再检查，避免误报
        body_no_comments = re.sub(r"//.*$", "", body, flags=re.MULTILINE)
        body_no_comments = re.sub(r"/\*.*?\*/", "", body_no_comments, flags=re.DOTALL)
        if re.search(r"[Mm]ock", body_no_comments):
            degrade_ok = False
            degrade_detail = "markNativeInitFailed() 方法体中包含 mock 降级逻辑"
        if re.search(r"environment\s*=\s*['\"]mock['\"]", body_no_comments):
            degrade_ok = False
            degrade_detail = "markNativeInitFailed() 方法体中将 environment 设为 mock"
    else:
        degrade_ok = False
        degrade_detail = "未找到 markNativeInitFailed() 方法定义"

    results.append((degrade_ok, f"AppContext.ets markNativeInitFailed() 不降级到 mock — {degrade_detail}"))

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

    # 查找 onCreate 方法体
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
# 检查 4: Index 必须调用 nativeBridge.initialize
# ---------------------------------------------------------------------------

def check_index_native_init(harmony_root: str) -> List[Tuple[bool, str]]:
    results: List[Tuple[bool, str]] = []
    fpath = os.path.join(harmony_root, "entry/src/main/ets/pages/Index.ets")
    content = read_file(fpath)

    if content is None:
        results.append((False, f"文件不存在: {fpath}"))
        return results

    # 查找 initAndLoad 方法体
    init_match = re.search(
        r"(?:async\s+)?initAndLoad\s*\([^)]*\)\s*(?::\s*[^{]+?)?\{",
        content,
    )
    if init_match is None:
        results.append((False, "Index.ets 未找到 initAndLoad() 方法"))
        return results

    start = init_match.end() - 1
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

    # 4a: 必须调用 nativeBridge.initialize(workspacePath)
    has_init = bool(re.search(r"nativeBridge\s*\.?\s*initialize\s*\(", body))
    results.append((
        has_init,
        f"Index.ets initAndLoad() 调用 nativeBridge.initialize() — {'存在' if has_init else '未找到'}",
    ))

    # 4b: 初始化失败时必须设置 errorMessage，不能静默降级
    # 检查 catch 或失败分支中是否有 errorMessage 赋值
    has_error_msg = bool(re.search(r"errorMessage\s*=", body))
    has_silent_degrade = False

    # 检查 catch 块中是否有 mock 降级（去除注释后检查）
    body_no_comments = re.sub(r"//.*$", "", body, flags=re.MULTILINE)
    body_no_comments = re.sub(r"/\*.*?\*/", "", body_no_comments, flags=re.DOTALL)
    catch_matches = re.findall(r"catch\s*\([^)]*\)\s*\{([^}]*(?:\{[^}]*\}[^}]*)*)\}", body_no_comments, re.DOTALL)
    for cm in catch_matches:
        if re.search(r"[Mm]ock", cm):
            has_silent_degrade = True

    error_ok = has_error_msg and not has_silent_degrade
    detail_parts = []
    if not has_error_msg:
        detail_parts.append("未找到 errorMessage 赋值")
    if has_silent_degrade:
        detail_parts.append("catch 块中存在 mock 降级")
    detail = "; ".join(detail_parts) if detail_parts else "正常"

    results.append((error_ok, f"Index.ets 初始化失败设置 errorMessage 且不静默降级 — {detail}"))

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

    # 6a: 提取 napi_init.cpp 中注册的函数名
    napi_path = os.path.join(harmony_root, "entry/src/main/cpp/napi_init.cpp")
    napi_content = read_file(napi_path)
    if napi_content is None:
        results.append((False, f"文件不存在: {napi_path}"))
        return results

    # 匹配 napi_define_properties 中的函数名描述符
    # 典型模式: { "nativeInit", nullptr, NativeInit, nullptr, nullptr, nullptr, napi_default, nullptr }
    # 或 napi_define_properties 调用中的属性描述
    napi_funcs = set()
    # 方式1: 在初始化数组中找 "nativeXxx" 字符串
    for m in re.finditer(r"\"(native\w+)\"", napi_content):
        napi_funcs.add(m.group(1))

    # 方式2: napi_set_property_name 等模式
    for m in re.finditer(r"napi_set_property_name\s*\([^,]+,\s*\"(\w+)\"", napi_content):
        napi_funcs.add(m.group(1))

    # 6b: 提取 NativeWriterCoreBridge.ets 中调用的函数名
    bridge_path = os.path.join(harmony_root, "entry/src/main/ets/bridge/NativeWriterCoreBridge.ets")
    bridge_content = read_file(bridge_path)
    if bridge_content is None:
        results.append((False, f"文件不存在: {bridge_path}"))
        return results

    # 匹配 this.getNativeModule().nativeXxx( 或 nativeModule.nativeXxx(
    arkts_funcs = set()
    for m in re.finditer(
        r"(?:getNativeModule|nativeModule)\s*\(\s*\)\s*\.\s*(native\w+)\s*\(",
        bridge_content,
    ):
        arkts_funcs.add(m.group(1))

    # 也匹配 this.nativeModule.nativeXxx(
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
    header_path = os.path.join(harmony_root, "entry/src/main/cpp/writer_core_bridge.h")
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
                r"#\[no_mangle\]\s*pub\s+unsafe\s+extern\s+\"C\"\s+fn\s+(writer_core_\w+)",
                rs_content,
            ):
                rust_funcs.add(m.group(1))
    else:
        results.append((False, f"Rust ffi 目录不存在: {ffi_dir}"))
        return results

    # 7c: 比对
    # C header 声明必须是 Rust FFI 导出的子集
    in_header_not_in_rust = header_funcs - rust_funcs
    in_rust_not_in_header = rust_funcs - header_funcs

    # C header 有但 Rust 缺失 → 错误
    ok = len(in_header_not_in_rust) == 0
    if ok:
        detail = f"C header 声明 {len(header_funcs)} 个函数, Rust FFI 导出 {len(rust_funcs)} 个函数, C header 是 Rust 子集"
    else:
        detail = f"C header 有但 Rust 缺失: {sorted(in_header_not_in_rust)}"

    results.append((ok, f"writer_core_bridge.h 声明是 Rust FFI 导出的子集 — {detail}"))

    # Rust 有但 C header 缺失 → 仅报告（可能是 Desktop 专用）
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
        ("2. AppContext 默认 native", check_app_context_native_default, harmony_root),
        ("3. EntryAbility 必须 setAbilityContext", check_entry_ability_set_context, harmony_root),
        ("4. Index 必须调用 nativeBridge.initialize", check_index_native_init, harmony_root),
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

    # 汇总
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
