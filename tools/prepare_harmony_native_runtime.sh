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


def symbol_identity(name):
    """Return base name, required/provided version, and default-version flag."""
    name = name.split()[0]
    if "@@" in name:
        base, version = name.rsplit("@@", 1)
        return base, version, True
    if "@" in name:
        base, version = name.rsplit("@", 1)
        return base, version, False
    return name, None, False


def symbol_matches(required_name, exported_name):
    required_base, required_version, _ = symbol_identity(required_name)
    exported_base, exported_version, exported_default = symbol_identity(exported_name)
    if required_base != exported_base:
        return False
    if required_version is not None:
        return required_version == exported_version
    return exported_version is None or exported_default


def same_symbol_base(required_name, exported_name):
    return symbol_identity(required_name)[0] == symbol_identity(exported_name)[0]


def dynamic_symbols(path):
    undefined = []
    exported = []
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
            undefined.append({"name": name, "bind": bind, "visibility": visibility})
        elif bind in ("GLOBAL", "WEAK", "GNU_UNIQUE") and visibility in ("DEFAULT", "PROTECTED"):
            exported.append({"name": name, "bind": bind, "visibility": visibility})
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


library_cache = {}


def inspect_library(path, category):
    path = os.path.realpath(path)
    if path in library_cache:
        return library_cache[path]
    with open(path, "rb") as library:
        if library.read(4) != b"\x7fELF":
            raise RuntimeError("not an ELF object")
    needed = needed_libraries(path)
    soname = None
    for line in run_readelf(["-d"], path).splitlines():
        match = re.search(r"Library soname:\s*\[([^\]]+)\]", line)
        if match:
            soname = match.group(1)
            break
    undefined, exported = dynamic_symbols(path)
    info = {
        "path": path,
        "name": os.path.basename(path),
        "soname": soname or os.path.basename(path),
        "category": category,
        "needed": needed,
        "undefined": undefined,
        "exported": exported,
    }
    library_cache[path] = info
    return info


def provider_catalog():
    catalog = {"package": {"by_name": {}, "by_soname": {}},
               "sdk": {"by_name": {}, "by_soname": {}}}
    invalid_package_files = {}
    package_root = os.path.realpath(prebuilt_dir)
    for root in search_roots:
        category = "package" if os.path.realpath(root) == package_root else "sdk"
        try:
            entries = list(os.scandir(root))
        except OSError:
            continue
        for entry in entries:
            if ".so" not in entry.name or not os.path.isfile(entry.path):
                continue
            path = os.path.realpath(entry.path)
            try:
                with open(path, "rb") as library:
                    if library.read(4) != b"\x7fELF":
                        if category == "package":
                            invalid_package_files[entry.name] = path
                        continue
                info = inspect_library(path, category)
            except (OSError, RuntimeError):
                if category == "package":
                    # A file with ELF magic that this readelf cannot inspect is
                    # insufficient evidence that the packaged dependency is
                    # invalid. Leave it unresolved instead of failing it.
                    pass
                continue
            catalog[category]["by_name"].setdefault(entry.name, info)
            catalog[category]["by_soname"].setdefault(info["soname"], info)
    return catalog, invalid_package_files


catalog, invalid_package_files = provider_catalog()


def resolve_needed(soname):
    # A broken file at the package soname shadows any SDK candidate with the
    # same name, so this is a provable packaging defect.
    if soname in invalid_package_files:
        return None, "invalid-package", invalid_package_files[soname]
    for category in ("package", "sdk"):
        provider = catalog[category]["by_name"].get(soname)
        if provider is None:
            provider = catalog[category]["by_soname"].get(soname)
        if provider is not None:
            return provider, category, None
    return None, "unknown", None


def matching_exports(required_name, libraries):
    matches = []
    for library in libraries:
        for exported in library["exported"]:
            if symbol_matches(required_name, exported["name"]):
                matches.append((library, exported))
    return matches


def display_symbol(symbol):
    return f"{symbol['bind']} {symbol['name']}"


targets = []
if napi_so:
    if os.path.isfile(napi_so):
        targets.append(napi_so)
    else:
        print(f"诊断未完成：CMake 目标库不存在：{napi_so}")
for name in ("libwriter_core_ffi.so", "libonnxruntime.so"):
    path = os.path.join(prebuilt_dir, name)
    if os.path.isfile(path):
        targets.append(path)

targets = list(dict.fromkeys(os.path.realpath(path) for path in targets))
if not targets:
    print(f"HarmonyOS ELF diagnostics inconclusive: no target libraries found in {prebuilt_dir}")
    raise SystemExit(0)

print("HarmonyOS dynamic dependency and symbol-provider diagnostics")
print(f"Provider search roots: {', '.join(search_roots) or '(none)'}")
confirmed_missing_packaged = []
inconclusive = False

for target in targets:
    print(f"\n[{os.path.basename(target)}] {target}")
    try:
        target_category = "package" if os.path.dirname(target) == os.path.realpath(prebuilt_dir) else "target"
        target_info = inspect_library(target, target_category)
        needed = target_info["needed"]
        print("  DT_NEEDED:")
        if not needed:
            print("    (none)")
        dependency_paths = []
        unresolved_needed = []
        seen_sonames = set()
        seen_paths = {target_info["path"]}
        queue = [(soname, target_info) for soname in needed]
        for soname in needed:
            provider, category, detail = resolve_needed(soname)
            if provider is not None:
                print(f"    {soname} -> {provider['path']} ({'随包 ELF' if category == 'package' else 'SDK ELF; 设备运行时未核实'})")
            elif category == "invalid-package":
                print(f"    {soname} -> 包内文件不是可读 ELF：{detail}")
                confirmed_missing_packaged.append((target, soname, detail))
            else:
                print(f"    {soname} -> 本地 package/SDK 未找到；设备系统提供者未知")
                unresolved_needed.append((soname, target_info))
                inconclusive = True

        while queue:
            soname, importer = queue.pop(0)
            if soname in seen_sonames:
                continue
            seen_sonames.add(soname)
            provider, category, detail = resolve_needed(soname)
            if category == "invalid-package":
                print(f"  确认缺失随包依赖：{importer['name']} 需要 {soname}，包内文件不是有效 ELF：{detail}")
                confirmed_missing_packaged.append((importer["path"], soname, detail))
                continue
            if provider is None:
                if not any(missing[0] == soname for missing in unresolved_needed):
                    unresolved_needed.append((soname, importer))
                inconclusive = True
                continue
            if provider["path"] in seen_paths:
                continue
            seen_paths.add(provider["path"])
            dependency_paths.append(provider)
            queue.extend((child, provider) for child in provider["needed"])

        closure = [target_info, *dependency_paths]
        local_candidates = list(library_cache.values())
        closure_paths = {library["path"] for library in closure}
        requirements = {}
        for library in closure:
            for symbol in library["undefined"]:
                identity = (library["path"], *symbol_identity(symbol["name"]))
                current = requirements.get(identity)
                if current is None or (current["bind"] == "WEAK" and symbol["bind"] != "WEAK"):
                    requirements[identity] = {**symbol, "importer": library}

        resolved_package = []
        resolved_sdk = []
        candidate_package = []
        candidate_sdk = []
        version_mismatches = []
        unknown_symbols = []
        for symbol in requirements.values():
            strong = symbol["bind"] != "WEAK"
            matches = matching_exports(symbol["name"], closure)
            package_matches = [(lib, exp) for lib, exp in matches if lib["category"] in ("package", "target")]
            sdk_matches = [(lib, exp) for lib, exp in matches if lib["category"] == "sdk"]
            if package_matches:
                if strong:
                    resolved_package.append((symbol, package_matches[0]))
                continue
            if sdk_matches:
                if strong:
                    resolved_sdk.append((symbol, sdk_matches[0]))
                continue
            if not strong:
                continue
            same_base = [(lib, exp) for lib in closure for exp in lib["exported"]
                         if same_symbol_base(symbol["name"], exp["name"])]
            if same_base:
                version_mismatches.append((symbol, same_base[0]))
                continue
            other_matches = matching_exports(symbol["name"], [lib for lib in local_candidates if lib["path"] not in closure_paths])
            other_package = [(lib, exp) for lib, exp in other_matches if lib["category"] == "package"]
            other_sdk = [(lib, exp) for lib, exp in other_matches if lib["category"] == "sdk"]
            if other_package:
                candidate_package.append((symbol, other_package[0]))
            elif other_sdk:
                candidate_sdk.append((symbol, other_sdk[0]))
            elif strong:
                unknown_symbols.append(symbol)

        print(f"  DT_NEEDED 闭包 ELF 的 dynamic undefined symbols: {sum(s['bind'] != 'WEAK' for s in requirements.values())} strong, {sum(s['bind'] == 'WEAK' for s in requirements.values())} weak (按导入库/名称/版本去重)")
        package_weak_exports = sum(exported["bind"] == "WEAK" for _, (_, exported) in resolved_package)
        sdk_weak_exports = sum(exported["bind"] == "WEAK" for _, (_, exported) in resolved_sdk)
        versioned_package = sum(symbol_identity(symbol["name"])[1] is not None for symbol, _ in resolved_package)
        versioned_sdk = sum(symbol_identity(symbol["name"])[1] is not None for symbol, _ in resolved_sdk)
        print(f"  随包/目标 ELF 已解析（在实际 DT_NEEDED 闭包内按名称/绑定/可见性/版本匹配）：{len(resolved_package)} strong symbol(s)")
        print(f"    带版本 {versioned_package}；GLOBAL/GNU_UNIQUE 导出满足 {len(resolved_package) - package_weak_exports}，WEAK 导出满足 {package_weak_exports}")
        print(f"  SDK ELF 匹配候选（静态 SDK 文件在闭包内提供；设备运行时未核实）：{len(resolved_sdk)} strong symbol(s)")
        print(f"    带版本 {versioned_sdk}；GLOBAL/GNU_UNIQUE 导出满足 {len(resolved_sdk) - sdk_weak_exports}，WEAK 导出满足 {sdk_weak_exports}")
        if resolved_sdk:
            inconclusive = True
        if version_mismatches:
            print("  同名导出存在但 ELF 符号版本不匹配（未计为已解析）：")
            for symbol, (provider, exported) in version_mismatches:
                print(f"    {symbol['importer']['name']}: {display_symbol(symbol)} -> {provider['name']} ({exported['bind']} {exported['visibility']} {exported['name']})")
            inconclusive = True
        if candidate_package:
            print("  包内候选提供者位于 DT_NEEDED 闭包之外（未证明会被加载）：")
            for symbol, (provider, exported) in candidate_package:
                print(f"    {symbol['importer']['name']}: {display_symbol(symbol)} -> {provider['name']} ({exported['bind']} {exported['visibility']} {exported['name']})")
            inconclusive = True
        if candidate_sdk:
            print("  SDK/系统候选提供者位于 DT_NEEDED 闭包之外（设备是否加载无法本机核实）：")
            for symbol, (provider, exported) in candidate_sdk:
                print(f"    {symbol['importer']['name']}: {display_symbol(symbol)} -> {provider['name']} ({exported['bind']} {exported['visibility']} {exported['name']})")
            inconclusive = True
        if unknown_symbols:
            print("  本地 package/SDK 均未找到 strong symbol；设备系统提供者未知，不能据此判定设备不兼容：")
            for symbol in unknown_symbols:
                print(f"    {symbol['importer']['name']}: {display_symbol(symbol)}")
            inconclusive = True
        weak_unprovided = [symbol for symbol in requirements.values()
                           if symbol["bind"] == "WEAK"
                           and not matching_exports(symbol["name"], closure)]
        if weak_unprovided:
            print(f"  闭包中没有 provider 的 weak import（不作为强缺失）：{len(weak_unprovided)}")
            for symbol in weak_unprovided:
                print(f"    {symbol['importer']['name']}: {display_symbol(symbol)}")
        if unresolved_needed:
            print("  未解析的 DT_NEEDED 本地依赖（可能由设备系统提供，需设备 ELF/日志核实）：")
            for soname, importer in unresolved_needed:
                print(f"    {importer['name']} -> {soname}")
    except (OSError, RuntimeError) as error:
        print(f"  诊断未完成：{error}；本地静态信息不足以判断设备兼容性。")
        inconclusive = True

if confirmed_missing_packaged:
    print("HarmonyOS ELF diagnostics failed: a DT_NEEDED entry points to an invalid packaged ELF dependency.")
    raise SystemExit(1)
if inconclusive:
    print("HarmonyOS ELF diagnostics inconclusive: device-side system providers and the unnamed relocation failure were not verified.")
else:
    print("HarmonyOS ELF diagnostics complete for the available package/SDK ELF set; this is not a device compatibility claim.")
PY
