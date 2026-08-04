#!/bin/bash
set -euo pipefail

usage() {
    echo "用法: $0 <apk_path> <expected_abis>"
    echo ""
    echo "参数:"
    echo "  apk_path       APK 文件路径"
    echo "  expected_abis  逗号分隔的期望 ABI 列表 (如 arm64-v8a,x86_64)"
    echo ""
    echo "示例:"
    echo "  $0 app.apk arm64-v8a"
    echo "  $0 app.apk arm64-v8a,x86_64"
    exit 1
}

if [ $# -lt 2 ]; then
    usage
fi

APK_PATH="$1"
EXPECTED_ABIS="$2"

if [ ! -f "$APK_PATH" ]; then
    echo "错误: APK 文件不存在: $APK_PATH"
    exit 1
fi

IFS=',' read -ra EXPECTED_ARRAY <<< "$EXPECTED_ABIS"
VALID_ABIS=("arm64-v8a" "x86_64" "universal")

for abi in "${EXPECTED_ARRAY[@]}"; do
    abi_trimmed=$(echo "$abi" | xargs)
    valid=false
    for valid_abi in "${VALID_ABIS[@]}"; do
        if [ "$abi_trimmed" = "$valid_abi" ]; then
            valid=true
            break
        fi
    done
    if [ "$valid" = false ]; then
        echo "错误: 不支持的期望 ABI '$abi_trimmed'"
        exit 1
    fi
done

echo "验证 APK ABI: $APK_PATH"
echo "期望 ABI: $EXPECTED_ABIS"

ALL_ERRORS=0

EXPECTED_ARM64=false
EXPECTED_X86=false
IS_UNIVERSAL=false
for abi in "${EXPECTED_ARRAY[@]}"; do
    abi_trimmed=$(echo "$abi" | xargs)
    if [ "$abi_trimmed" = "arm64-v8a" ]; then
        EXPECTED_ARM64=true
    elif [ "$abi_trimmed" = "x86_64" ]; then
        EXPECTED_X86=true
    elif [ "$abi_trimmed" = "universal" ]; then
        IS_UNIVERSAL=true
        EXPECTED_ARM64=true
        EXPECTED_X86=true
    fi
done

check_elf_arch() {
    local so_path="$1"
    local expected_abi="$2"
    local expected_machine=""

    case "$expected_abi" in
        arm64-v8a)
            expected_machine="AArch64"
            ;;
        x86_64)
            expected_machine="X86-64"
            ;;
    esac

    if command -v readelf &> /dev/null; then
        local machine
        machine=$(LANG=C readelf -h "$so_path" 2>/dev/null | grep -i "machine\|架构" | head -1 || true)
        if [ -z "$machine" ]; then
            echo "  警告: 无法读取 ELF header: $so_path"
            return 1
        fi
        if echo "$machine" | grep -qi "$expected_machine"; then
            echo "  ELF 架构验证通过: $expected_abi -> $expected_machine"
            return 0
        else
            echo "  错误: ELF 架构不匹配 (期望 $expected_machine): $machine"
            return 1
        fi
    elif command -v file &> /dev/null; then
        local file_info
        file_info=$(file "$so_path" 2>/dev/null || true)
        local expected_pattern=""
        case "$expected_abi" in
            arm64-v8a) expected_pattern="AArch64\|aarch64\|ARM aarch64" ;;
            x86_64) expected_pattern="x86-64\|X86-64" ;;
        esac
        if echo "$file_info" | grep -qi "$expected_pattern"; then
            echo "  ELF 架构验证通过 (file): $expected_abi"
            return 0
        else
            echo "  错误: ELF 架构不匹配 (file): $file_info"
            return 1
        fi
    else
        echo "  警告: readelf 和 file 均不可用，跳过 ELF 架构验证"
        return 0
    fi
}

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "提取 .so 文件进行 ELF 验证..."
ELF_VERIFY_ABIS=()
for abi in "${EXPECTED_ARRAY[@]}"; do
    abi_trimmed=$(echo "$abi" | xargs)
    if [ "$abi_trimmed" = "universal" ]; then
        ELF_VERIFY_ABIS+=("arm64-v8a" "x86_64")
    else
        ELF_VERIFY_ABIS+=("$abi_trimmed")
    fi
done

for abi in "${ELF_VERIFY_ABIS[@]}"; do
    so_in_apk="lib/$abi/libuniffi_writer_core.so"
    if unzip -o "$APK_PATH" "$so_in_apk" -d "$TMPDIR" &>/dev/null; then
        check_elf_arch "$TMPDIR/$so_in_apk" "$abi" || ALL_ERRORS=$((ALL_ERRORS + 1))
    else
        echo "  警告: 无法从 APK 提取 $so_in_apk"
    fi
done

echo ""
echo "验证 APK 内 .so 文件存在性..."

check_abi_in_apk() {
    local abi="$1"
    local should_exist="$2"
    local so_path="lib/$abi/libuniffi_writer_core.so"

    local found=false
    local listing
    listing=$(unzip -l "$APK_PATH" 2>/dev/null || true)
    if echo "$listing" | grep -q "$so_path"; then
        found=true
    fi

    if [ "$should_exist" = "true" ]; then
        if [ "$found" = "true" ]; then
            echo "  ✓ $so_path 存在"
        else
            echo "  ✗ 错误: $so_path 应存在但未找到"
            ALL_ERRORS=$((ALL_ERRORS + 1))
        fi
    else
        if [ "$found" = "true" ]; then
            echo "  ✗ 错误: $so_path 不应存在但找到了"
            ALL_ERRORS=$((ALL_ERRORS + 1))
        else
            echo "  ✓ $so_path 不存在 (正确)"
        fi
    fi
}

check_abi_in_apk "arm64-v8a" "$EXPECTED_ARM64"
check_abi_in_apk "x86_64" "$EXPECTED_X86"

echo ""
echo "检查非预期 ABI..."
APK_LISTING=$(unzip -l "$APK_PATH" 2>/dev/null || true)
ALL_SO_ENTRIES=$(echo "$APK_LISTING" | grep -oP 'lib/\K[^/]+(?=/libuniffi_writer_core\.so)' || true)
if [ -n "$ALL_SO_ENTRIES" ]; then
    while IFS= read -r abi_found; do
        if [ -z "$abi_found" ]; then
            continue
        fi
        is_expected=false
        for expected_abi in "${ELF_VERIFY_ABIS[@]}"; do
            if [ "$abi_found" = "$expected_abi" ]; then
                is_expected=true
                break
            fi
        done
        if [ "$is_expected" = false ]; then
            echo "  ✗ 错误: 发现非预期 ABI: $abi_found"
            ALL_ERRORS=$((ALL_ERRORS + 1))
        fi
    done <<< "$ALL_SO_ENTRIES"
fi

echo ""
if [ "$ALL_ERRORS" -eq 0 ]; then
    echo "APK ABI 验证通过 ✓"
    exit 0
else
    echo "APK ABI 验证失败，共 $ALL_ERRORS 个错误 ✗"
    exit 1
fi
