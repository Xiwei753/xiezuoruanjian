#!/usr/bin/env bash
# =============================================================================
# check_harmony_bridge_consistency.sh
# =============================================================================
#
# 检查 Harmony 桥接链路一致性：
#   ArkTS 调用的 nativeXXX 必须在 napi_*.cpp 注册
#   napi_*.cpp 调用的 writer_core_xxx 必须在 writer_core_bridge.h 声明
#   writer_core_bridge.h 声明的函数必须在 Rust FFI 存在
#
# 用法: bash scripts/check_harmony_bridge_consistency.sh
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

HARMONY_CPP="$PROJECT_ROOT/apps/harmony/entry/src/main/cpp"
NAPI_SOURCES=("$HARMONY_CPP"/corebridge/napi/napi_*.cpp)
BRIDGE_H="$HARMONY_CPP/corebridge/writer_core_bridge.h"
FFI_DIR="$PROJECT_ROOT/core/writer_core/src/ffi"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

errors=0
warnings=0

echo "=== Harmony Bridge Consistency Check ==="
echo ""

# ── Step 1: Extract native function names from napi_*.cpp registration ──
echo "Step 1: Extracting native function names from napi_*.cpp..."
napi_registered=()
for napi_source in "${NAPI_SOURCES[@]}"; do
    [[ -f "$napi_source" ]] || continue
    while IFS= read -r line; do
        if [[ "$line" =~ \"(native[A-Za-z]+)\" ]]; then
            napi_registered+=("${BASH_REMATCH[1]}")
        fi
    done < "$napi_source"
done

echo "  Found ${#napi_registered[@]} registered NAPI functions"

# ── Step 2: Extract writer_core_* declarations from C header ──
echo "Step 2: Extracting writer_core_* declarations from writer_core_bridge.h..."
header_declared=()
while IFS= read -r line; do
    if [[ "$line" =~ writer_core_([a-z_]+)\( ]]; then
        header_declared+=("writer_core_${BASH_REMATCH[1]}")
    fi
done < "$BRIDGE_H"

echo "  Found ${#header_declared[@]} declared C functions"

# ── Step 3: Extract #[no_mangle] Rust FFI functions ──
echo "Step 3: Extracting #[no_mangle] Rust FFI functions..."
rust_exported=()
for rs_file in "$FFI_DIR"/*.rs; do
    while IFS= read -r line; do
        if [[ "$line" =~ fn\ (writer_core_[a-z_]+)\( ]]; then
            rust_exported+=("${BASH_REMATCH[1]}")
        fi
    done < "$rs_file"
done

echo "  Found ${#rust_exported[@]} exported Rust FFI functions"

# ── Step 4: Check NAPI → C header consistency ──
echo "Step 4: Checking NAPI → C header consistency..."

# Extract the actual writer_core_* calls from all modular NAPI sources.
napi_c_calls=()
for napi_source in "${NAPI_SOURCES[@]}"; do
    [[ -f "$napi_source" ]] || continue
    while IFS= read -r line; do
        if [[ "$line" =~ writer_core_([a-z_]+)\( ]]; then
            napi_c_calls+=("writer_core_${BASH_REMATCH[1]}")
        fi
    done < "$napi_source"
done

for c_call in "${napi_c_calls[@]}"; do
    found=false
    for declared in "${header_declared[@]}"; do
        if [[ "$declared" == "$c_call" ]]; then
            found=true
            break
        fi
    done

    if ! $found; then
        echo -e "  ${RED}ERROR${NC}: napi_*.cpp calls '${c_call}' but it's NOT declared in writer_core_bridge.h"
        errors=$((errors + 1))
    fi
done

# ── Step 5: Check C header → Rust FFI consistency ──
echo "Step 5: Checking C header → Rust FFI consistency..."

for c_func in "${header_declared[@]}"; do
    found=false
    for rust_func in "${rust_exported[@]}"; do
        if [[ "$rust_func" == "$c_func" ]]; then
            found=true
            break
        fi
    done

    if ! $found; then
        echo -e "  ${RED}ERROR${NC}: C header declares '${c_func}' but it's NOT exported in Rust FFI"
        errors=$((errors + 1))
    fi
done

# ── Step 6: Check Rust FFI → C header consistency ──
echo "Step 6: Checking Rust FFI → C header consistency..."

for rust_func in "${rust_exported[@]}"; do
    found=false
    for c_func in "${header_declared[@]}"; do
        if [[ "$c_func" == "$rust_func" ]]; then
            found=true
            break
        fi
    done

    if ! $found; then
        echo -e "  ${YELLOW}WARNING${NC}: Rust FFI exports '${rust_func}' but it's NOT declared in writer_core_bridge.h (may be internal)"
        warnings=$((warnings + 1))
    fi
done

# ── Step 7: Check ArkTS native calls → NAPI registration ──
echo "Step 7: Checking ArkTS native calls → NAPI registration..."

arkts_ets_dir="$PROJECT_ROOT/apps/harmony/entry/src/main/ets"
if [[ -d "$arkts_ets_dir" ]]; then
    while IFS= read -r -d '' ets_file; do
        while IFS= read -r line; do
            if [[ "$line" =~ \.native([A-Za-z]+)\( ]]; then
                arkts_call="native${BASH_REMATCH[1]}"
                found=false
                for napi_func in "${napi_registered[@]}"; do
                    if [[ "$napi_func" == "$arkts_call" ]]; then
                        found=true
                        break
                    fi
                done
                if ! $found; then
                    echo -e "  ${RED}ERROR${NC}: ${ets_file##*/} calls '${arkts_call}' but it's NOT registered in napi_*.cpp"
                    errors=$((errors + 1))
                fi
            fi
        done < "$ets_file"
    done < <(find "$arkts_ets_dir" -name "*.ets" -print0)
fi

# ── Step 8: Check DTO field name consistency (FFI JSON vs CoreDtos.ets) ──
echo "Step 8: Checking DTO field name consistency (FFI JSON vs CoreDtos.ets)..."

# 注意：原 model/CoreDtos.ets 已按 Issue #629 第3节拆分到 corebridge/dto/ 下多个文件
# (ProjectDtos.ets, EditorDtos.ets, SettingsDtos.ets, SyncDtos.ets, StarMapDtos.ets,
#  PlatformDtos.ets, ResultEnvelope.ets)。下面的字段一致性检查只关心 Rust FFI 是否
# 输出错误字段名（"name" vs "title"、"editedAt" vs "timestamp"），与具体哪个 .ets
# 文件无关，因此只要 dto/ 目录存在即触发检查。
core_dtos="$PROJECT_ROOT/apps/harmony/entry/src/main/ets/corebridge/dto"

if [[ -d "$core_dtos" ]]; then
    # Check that FFI does NOT output "name" for Project/Volume/Chapter
    # (CoreDtos.ets uses "title" for these entities)
    name_count=0
    for rs_file in "$FFI_DIR"/*.rs; do
        while IFS= read -r line; do
            # Match "name": xxx.title patterns in JSON construction
            if [[ "$line" =~ \"name\":.*\.title ]]; then
                echo -e "  ${RED}ERROR${NC}: ${rs_file##*/} outputs \"name\" for a .title field — should be \"title\" to match CoreDtos.ets"
                errors=$((errors + 1))
                name_count=$((name_count + 1))
            fi
        done < "$rs_file"
    done

    if [[ $name_count -eq 0 ]]; then
        echo "  No 'name' → .title mismatches found (good)"
    fi

    # Check that FFI does NOT output "editedAt" for RecentEdit
    # (CoreDtos.ets uses "timestamp" for RecentEdit)
    edited_at_count=0
    for rs_file in "$FFI_DIR"/*.rs; do
        while IFS= read -r line; do
            if [[ "$line" =~ \"editedAt\":.*\.timestamp ]]; then
                echo -e "  ${RED}ERROR${NC}: ${rs_file##*/} outputs \"editedAt\" for a .timestamp field — should be \"timestamp\" to match CoreDtos.ets"
                errors=$((errors + 1))
                edited_at_count=$((edited_at_count + 1))
            fi
        done < "$rs_file"
    done

    if [[ $edited_at_count -eq 0 ]]; then
        echo "  No 'editedAt' → .timestamp mismatches found (good)"
    fi
fi
echo ""
echo "=== Summary ==="
echo -e "  Errors:   ${RED}${errors}${NC}"
echo -e "  Warnings: ${YELLOW}${warnings}${NC}"

if [[ $errors -gt 0 ]]; then
    echo ""
    echo -e "${RED}Bridge consistency check FAILED${NC}"
    exit 1
else
    echo ""
    echo -e "${GREEN}Bridge consistency check PASSED${NC}"
    exit 0
fi
