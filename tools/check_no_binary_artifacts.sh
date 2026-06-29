#!/usr/bin/env bash
# check_no_binary_artifacts.sh — 守卫脚本：禁止提交构建产物/二进制文件
#
# 用途：
#   - CI 中作为 static analysis 的一步
#   - 本地开发时手动运行
#   - pre-commit hook 中调用
#
# 检查规则：
#   - 禁止提交以下扩展名的文件：*.apk *.aab *.hap *.exe *.msi *.AppImage *.zip *.7z *.dmg *.so *.dll
#   - 禁止提交 target/ build/ dist/ 目录
#   - 跳过 .gitignore 本身和此脚本
#
# 退出码：
#   0 = 通过（无违规文件）
#   1 = 失败（发现违规文件）

set -euo pipefail

FORBIDDEN_EXTENSIONS="apk aab hap exe msi AppImage zip 7z dmg so dll"
FORBIDDEN_DIRS="target build dist"

# 只检查 git 跟踪的文件（不检查 .gitignore 忽略的文件）
if ! git rev-parse --is-inside-work-tree > /dev/null 2>&1; then
    echo "ERROR: not inside a git repository"
    exit 1
fi

# 获取 git 跟踪的文件列表
tracked_files=$(git ls-files)

if [ -z "$tracked_files" ]; then
    echo "OK: no tracked files to check"
    exit 0
fi

violations=0

# 检查禁止的扩展名
for ext in $FORBIDDEN_EXTENSIONS; do
    # 使用 grep 过滤匹配的文件
    matching=$(echo "$tracked_files" | grep -i "\\.${ext}$" || true)
    if [ -n "$matching" ]; then
        echo "VIOLATION: found *.${ext} files (forbidden binary artifact):"
        echo "$matching" | while read -r f; do
            echo "  - $f"
        done
        violations=$((violations + 1))
    fi
done

# 检查禁止的目录
for dir in $FORBIDDEN_DIRS; do
    # 检查是否有文件在这些目录下
    matching=$(echo "$tracked_files" | grep "^${dir}/" || true)
    if [ -n "$matching" ]; then
        echo "VIOLATION: found files in ${dir}/ directory (forbidden build output):"
        echo "$matching" | head -5 | while read -r f; do
            echo "  - $f"
        done
        total=$(echo "$matching" | wc -l)
        if [ "$total" -gt 5 ]; then
            echo "  ... and $((total - 5)) more"
        fi
        violations=$((violations + 1))
    fi
done

if [ "$violations" -gt 0 ]; then
    echo ""
    echo "FAIL: $violations violation(s) found. Build artifacts must not be committed."
    echo "These files should be added to .gitignore and removed from git tracking."
    exit 1
else
    echo "OK: no forbidden binary artifacts or build directories found in tracked files"
    exit 0
fi
