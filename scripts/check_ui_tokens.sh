#!/usr/bin/env bash
# =============================================================================
# check_ui_tokens.sh — UI token 静态检查
# =============================================================================
#
# 检查内容：
#   1. QML 中无新增硬编码圆角（radius: <数字>，排除已知例外）
#   2. QML 中无新增硬编码阴影颜色（Qt.rgba(0,0,0,0.x) border hack）
#   3. Android 中无新增硬编码 FAB 底部避让（layout_margin="16dp" 在 FAB 上）
#   4. monetColor 不再扩展（不在新文件中出现）
#
# 返回值：0=通过，1=有违规
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

ERRORS=0

echo "=== UI Token Static Check ==="
echo ""

# --- Check 1: QML hardcoded radius ---
echo "1. Checking QML hardcoded radius (excluding known exceptions)..."
QML_RADIUS_ISSUES=""
while IFS= read -r line; do
    FILE=$(echo "$line" | cut -d: -f1)
    LINENUM=$(echo "$line" | cut -d: -f2)
    # Skip DesignTokens.qml itself
    if [[ "$FILE" == *"DesignTokens.qml" ]]; then continue; fi
    # Skip AppShadow.qml
    if [[ "$FILE" == *"AppShadow.qml" ]]; then continue; fi
    # Skip cursor radius (radius: 1 in WritingWorkspace)
    if [[ "$line" == *"radius: 1"* ]]; then continue; fi
    # Skip radius: 0 (valid)
    if [[ "$line" == *"radius: 0"* ]]; then continue; fi
    # Skip if already using dt. prefix
    if [[ "$line" == *"dt.radius"* ]] || [[ "$line" == *"dt.cardRadius"* ]] || [[ "$line" == *"dt.dialogRadius"* ]] || [[ "$line" == *"dt.fabRadius"* ]] || [[ "$line" == *"dt.inputFieldRadius"* ]]; then continue; fi
    QML_RADIUS_ISSUES="${QML_RADIUS_ISSUES}  $line"$'\n'
done < <(cd "$REPO_ROOT" && grep -rn 'radius: [0-9]' apps/desktop/qml/ 2>/dev/null || true)

if [[ -n "$QML_RADIUS_ISSUES" ]]; then
    echo "   FAIL: Found hardcoded radius in QML:"
    echo "$QML_RADIUS_ISSUES"
    ERRORS=$((ERRORS + 1))
else
    echo "   PASS"
fi

# --- Check 2: QML hardcoded shadow color ---
echo "2. Checking QML hardcoded shadow color (Qt.rgba(0,0,0,...) border hack)..."
QML_SHADOW_ISSUES=""
while IFS= read -r line; do
    FILE=$(echo "$line" | cut -d: -f1)
    # Skip AppShadow.qml and DesignTokens.qml
    if [[ "$FILE" == *"AppShadow.qml" ]] || [[ "$FILE" == *"DesignTokens.qml" ]]; then continue; fi
    QML_SHADOW_ISSUES="${QML_SHADOW_ISSUES}  $line"$'\n'
done < <(cd "$REPO_ROOT" && grep -rn 'Qt.rgba(0,0,0,' apps/desktop/qml/ 2>/dev/null || true)

if [[ -n "$QML_SHADOW_ISSUES" ]]; then
    echo "   FAIL: Found hardcoded shadow color in QML:"
    echo "$QML_SHADOW_ISSUES"
    ERRORS=$((ERRORS + 1))
else
    echo "   PASS"
fi

# --- Check 3: Android hardcoded FAB bottom margin ---
echo "3. Checking Android hardcoded FAB bottom margin..."
FAB_MARGIN_ISSUES=""
while IFS= read -r line; do
    FILE=$(echo "$line" | cut -d: -f1)
    # Only check FAB elements
    if [[ "$line" == *"FloatingActionButton"* ]] || [[ "$line" == *"fab"* ]]; then
        if [[ "$line" == *"layout_margin"* ]] && [[ "$line" != *"FabPlacementHelper"* ]]; then
            FAB_MARGIN_ISSUES="${FAB_MARGIN_ISSUES}  $line"$'\n'
        fi
    fi
done < <(cd "$REPO_ROOT" && grep -rn 'layout_margin' apps/android/app/src/main/res/layout/ 2>/dev/null || true)

if [[ -n "$FAB_MARGIN_ISSUES" ]]; then
    echo "   WARN: Found FAB layout_margin in XML (should use FabPlacementHelper):"
    echo "$FAB_MARGIN_ISSUES"
    # This is a warning, not a hard error, since XML fallback is acceptable
else
    echo "   PASS"
fi

# --- Check 4: monetColor expansion ---
echo "4. Checking monetColor is not used in new files..."
MONET_ISSUES=""
while IFS= read -r line; do
    FILE=$(echo "$line" | cut -d: -f1)
    # Skip known legacy files
    if [[ "$FILE" == *"Models.kt" ]] || [[ "$FILE" == *"BridgeMappers.kt" ]] || [[ "$FILE" == *"MainActivity.kt" ]] || [[ "$FILE" == *"SettingsRepository.kt" ]]; then continue; fi
    if [[ "$FILE" == *"settings.rs" ]] || [[ "$FILE" == *"settings_ops.rs" ]] || [[ "$FILE" == *"api.udl" ]]; then continue; fi
    if [[ "$FILE" == *"DesignTokens.qml" ]] || [[ "$FILE" == *"main.qml" ]]; then continue; fi
    if [[ "$FILE" == *"CoreDtos.ets" ]] || [[ "$FILE" == *"MockWriterCoreBridge.ets" ]]; then continue; fi
    if [[ "$FILE" == *"HarmonyThemeAdapter.ets" ]]; then continue; fi
    MONET_ISSUES="${MONET_ISSUES}  $line"$'\n'
done < <(cd "$REPO_ROOT" && grep -rn 'monetColor\|monet_color' apps/ core/ 2>/dev/null | grep -v 'deprecated\|Deprecated\|legacy\|Legacy\|compat\|backward' || true)

if [[ -n "$MONET_ISSUES" ]]; then
    echo "   WARN: monetColor found in non-legacy files (should use theme_palette instead):"
    echo "$MONET_ISSUES"
else
    echo "   PASS"
fi

echo ""
if [[ $ERRORS -gt 0 ]]; then
    echo "=== UI Token Check FAILED ($ERRORS errors) ==="
    exit 1
else
    echo "=== UI Token Check PASSED ==="
    exit 0
fi
