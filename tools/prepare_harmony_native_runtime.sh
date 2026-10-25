#!/usr/bin/env bash
set -euo pipefail

MODE="prepare"
NATIVE_DIR=""
PREBUILT_DIR=""
NAPI_SO=""
SYSROOT=""

if [ "${1:-}" = "--diagnose-only" ]; then
    MODE="diagnose"
    if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
        echo "用法：$0 --diagnose-only <arm64 prebuilt 目录> <writer_core_napi.so> [HarmonyOS sysroot]" >&2
        exit 2
    fi
    PREBUILT_DIR="$2"
    NAPI_SO="$3"
    SYSROOT="${4:-}"
else
    if [ "$#" -ne 2 ]; then
        echo "用法：$0 <HarmonyOS Native SDK 目录> <arm64 prebuilt 目录>" >&2
        exit 2
    fi
    NATIVE_DIR="$1"
    PREBUILT_DIR="$2"
    SYSROOT="$NATIVE_DIR/sysroot"

    CXX_SHARED_SRC="$NATIVE_DIR/llvm/lib/aarch64-linux-ohos/c++/libc++_shared.so"
    CXX_SHARED_DST="$PREBUILT_DIR/libc++_shared.so"

    if [ ! -f "$CXX_SHARED_SRC" ]; then
        echo "错误：当前 HarmonyOS Native SDK 缺少 arm64 libc++_shared.so：$CXX_SHARED_SRC" >&2
        exit 1
    fi

    mkdir -p "$PREBUILT_DIR"
    cp -- "$CXX_SHARED_SRC" "$CXX_SHARED_DST"
    if ! cmp -s "$CXX_SHARED_SRC" "$CXX_SHARED_DST"; then
        echo "错误：libc++_shared.so 未能从 SDK 完整复制到 prebuilt 目录。" >&2
        exit 1
    fi

    echo "HarmonyOS arm64 libc++_shared.so: $CXX_SHARED_SRC -> $CXX_SHARED_DST"
fi

# Prefer the readelf shipped with the same HarmonyOS Native SDK. CMake invokes
# diagnose-only after linking NAPI and passes CMAKE_SYSROOT, whose parent is the
# Native SDK directory. Other callers can supply LLVM_READELF explicitly.
READ_ELF="${LLVM_READELF:-}"
if [ -z "$READ_ELF" ] && [ -n "$NATIVE_DIR" ] && [ -x "$NATIVE_DIR/llvm/bin/llvm-readelf" ]; then
    READ_ELF="$NATIVE_DIR/llvm/bin/llvm-readelf"
fi
if [ -z "$READ_ELF" ] && [ -n "$SYSROOT" ]; then
    SDK_FROM_SYSROOT="$(cd "$(dirname "$SYSROOT")" && pwd)"
    if [ -x "$SDK_FROM_SYSROOT/llvm/bin/llvm-readelf" ]; then
        READ_ELF="$SDK_FROM_SYSROOT/llvm/bin/llvm-readelf"
    fi
fi
if [ -z "$READ_ELF" ] && command -v llvm-readelf >/dev/null 2>&1; then
    READ_ELF="$(command -v llvm-readelf)"
fi
if [ -z "$READ_ELF" ] && command -v readelf >/dev/null 2>&1; then
    READ_ELF="$(command -v readelf)"
fi
if [ -z "$READ_ELF" ] || [ ! -x "$READ_ELF" ]; then
    echo "警告：未找到 llvm-readelf/readelf；跳过 HarmonyOS 动态依赖诊断。" >&2
    exit 0
fi

if [ "$MODE" = "diagnose" ]; then
    echo "HarmonyOS ELF reader: $READ_ELF"
else
    echo "HarmonyOS ELF reader: $READ_ELF"
    echo "检查现有 ONNX Runtime 与 Rust FFI；NAPI 将在 CMake 链接完成后再次诊断。"
fi

python3 - "$READ_ELF" "$PREBUILT_DIR" "$NAPI_SO" "$SYSROOT" "$NATIVE_DIR" <<'PY'
import os
import re
import subprocess
import sys

readelf, prebuilt_dir, napi_so, sysroot, native_dir = sys.argv[1:]


def run_readelf(args, path):
    completed = subprocess.run(
        [readelf, *args, path],
        check=False,
        capture_output=True,
        text=True,
        errors="replace",
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(f"{readelf} {' '.join(args)} {path}: {detail}")
    return completed.stdout


def needed_libraries(path):
    result = []
    for line in run_readelf(["-d"], path).splitlines():
        match = re.search(r"Shared library:\s*\[([^\]]+)\]", line)
        if match:
            result.append(match.group(1))
    return result


def dynamic_symbols(path):
    undefined = []
    exported = set()
    in_dynsym = False
    found_dynsym = False
    for line in run_readelf(["-Ws"], path).splitlines():
        if "Symbol table '.dynsym'" in line:
            in_dynsym = True
            found_dynsym = True
            continue
        if line.startswith("Symbol table '"):
            in_dynsym = False
        if not in_dynsym:
            continue
        fields = line.split()
        if len(fields) < 8 or not fields[0].endswith(":"):
            continue
        bind, visibility, section, name = fields[4], fields[5], fields[6], fields[7]
        if not name:
            continue
        if section == "UND":
            undefined.append((name, bind))
        elif bind in ("GLOBAL", "WEAK", "GNU_UNIQUE") and visibility in ("DEFAULT", "PROTECTED"):
            exported.add(name)
    if not found_dynsym:
        print(f"  警告：{path} 没有 .dynsym 符号表。")
    return undefined, exported


def sdk_roots():
    roots = [prebuilt_dir]
    if sysroot:
        roots.extend([
            os.path.join(sysroot, "usr", "lib", "aarch64-linux-ohos"),
            os.path.join(sysroot, "usr", "lib"),
        ])
    if native_dir:
        roots.extend([
            os.path.join(native_dir, "llvm", "lib", "aarch64-linux-ohos", "c++"),
            os.path.join(native_dir, "llvm", "lib", "aarch64-linux-ohos"),
        ])
    elif sysroot:
        sdk_dir = os.path.dirname(os.path.abspath(sysroot))
        roots.extend([
            os.path.join(sdk_dir, "llvm", "lib", "aarch64-linux-ohos", "c++"),
            os.path.join(sdk_dir, "llvm", "lib", "aarch64-linux-ohos"),
        ])
    return list(dict.fromkeys(root for root in roots if os.path.isdir(root)))


search_roots = sdk_roots()


def resolve_needed(soname):
    for root in search_roots:
        candidate = os.path.join(root, soname)
        if os.path.isfile(candidate):
            return os.path.realpath(candidate)
    return None


targets = []
if napi_so:
    if os.path.isfile(napi_so):
        targets.append(napi_so)
    else:
        print(f"警告：CMake 目标库不存在，无法诊断：{napi_so}")
for name in ("libwriter_core_ffi.so", "libonnxruntime.so"):
    path = os.path.join(prebuilt_dir, name)
    if os.path.isfile(path):
        targets.append(path)

if not targets:
    print(f"HarmonyOS ELF diagnostics: no target libraries found in {prebuilt_dir}")
    raise SystemExit(0)

print("HarmonyOS dynamic dependency and symbol-provider diagnostics")
print(f"Provider search roots: {', '.join(search_roots) or '(none)'}")

for target in targets:
    print(f"\n[{os.path.basename(target)}] {target}")
    try:
        needed = needed_libraries(target)
        print("  DT_NEEDED:")
        if not needed:
            print("    (none)")
        for soname in needed:
            provider = resolve_needed(soname)
            print(f"    {soname} -> {provider if provider else 'not found in package/SDK search roots'}")

        dependency_paths = []
        unresolved_needed = []
        queue = list(needed)
        seen_sonames = set()
        seen_paths = set()
        while queue:
            soname = queue.pop(0)
            if soname in seen_sonames:
                continue
            seen_sonames.add(soname)
            provider = resolve_needed(soname)
            if provider is None:
                unresolved_needed.append(soname)
                continue
            if provider in seen_paths:
                continue
            seen_paths.add(provider)
            dependency_paths.append(provider)
            queue.extend(needed_libraries(provider))

        provider_symbols = {}
        for provider in dependency_paths:
            _, exports = dynamic_symbols(provider)
            for symbol in exports:
                provider_symbols.setdefault(symbol, set()).add(os.path.basename(provider))

        undefined, _ = dynamic_symbols(target)
        strong_symbols = {name for name, bind in undefined if bind != "WEAK"}
        weak_symbols = {name for name, bind in undefined if bind == "WEAK"}
        provider_counts = {}
        missing_strong = []
        missing_weak = []
        for name in sorted(strong_symbols):
            matches = provider_symbols.get(name)
            if not matches:
                missing_strong.append(name)
            else:
                for provider in matches:
                    provider_counts[provider] = provider_counts.get(provider, 0) + 1
        for name in sorted(weak_symbols):
            if not provider_symbols.get(name):
                missing_weak.append(name)

        print(f"  Dynamic undefined symbols: {len(strong_symbols)} strong, {len(weak_symbols)} weak")
        if provider_counts:
            print("  Matching providers in DT_NEEDED closure:")
            for provider, count in sorted(provider_counts.items()):
                print(f"    {provider}: {count} strong symbol(s)")
        print(f"  Strong symbols without a matching package/SDK provider: {len(missing_strong)}")
        for name in missing_strong:
            print(f"    {name}")
        if missing_weak:
            print(f"  Weak symbols without a matching package/SDK provider: {len(missing_weak)}")
            for name in missing_weak:
                print(f"    {name}")
        if unresolved_needed:
            print("  DT_NEEDED entries without a package/SDK provider:")
            for soname in sorted(set(unresolved_needed)):
                print(f"    {soname}")
    except (OSError, RuntimeError) as error:
        print(f"  诊断失败：{error}")
PY
