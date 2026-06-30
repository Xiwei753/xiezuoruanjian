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
    # Skip radius: 3 (slider track internal detail, too small to be a UI token)
    if [[ "$line" == *"radius: 3"* ]]; then continue; fi
    # Skip if already using dt. prefix
    if [[ "$line" == *"dt.radius"* ]] || [[ "$line" == *"dt.cardRadius"* ]] || [[ "$line" == *"dt.dialogRadius"* ]] || [[ "$line" == *"dt.fabRadius"* ]] || [[ "$line" == *"dt.inputFieldRadius"* ]]; then continue; fi
    # Skip JS object literal radius (e.g. { radius: 30 } in StarMapGraphController)
    # Only flag QML Rectangle property radius (indented "radius: N" inside Rectangle block)
    if [[ "$line" == *"{ radius:"* ]] || [[ "$line" == *", radius:"* ]]; then continue; fi
    # Skip small decorative dots (e.g. width: 8; height: 8; radius: 4 — circular dot, not UI radius)
    if [[ "$line" == *"width: 8"* ]] && [[ "$line" == *"height: 8"* ]]; then continue; fi
    # Skip small circular dots in ProjectCard.qml (8x8 dot with radius: 4)
    if [[ "$FILE" == *"ProjectCard.qml" ]] && [[ "$line" == *"radius: 4"* ]]; then continue; fi
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

# --- Check 3: QML hardcoded hex colors ---
echo "3. Checking QML hardcoded hex colors (excluding DesignTokens.qml and AppShadow.qml)..."
QML_HEX_COLOR_ISSUES=""
while IFS= read -r line; do
    FILE=$(echo "$line" | cut -d: -f1)
    # Skip DesignTokens.qml and AppShadow.qml (whitelisted)
    if [[ "$FILE" == *"DesignTokens.qml" ]] || [[ "$FILE" == *"AppShadow.qml" ]]; then continue; fi
    # Skip "transparent" — structural, not a color value
    if [[ "$line" == *"\"transparent\""* ]]; then continue; fi
    QML_HEX_COLOR_ISSUES="${QML_HEX_COLOR_ISSUES}  $line"$'\n'
done < <(cd "$REPO_ROOT" && grep -rn '"#[0-9a-fA-F]\{6,8\}"' apps/desktop/qml/ 2>/dev/null || true)

if [[ -n "$QML_HEX_COLOR_ISSUES" ]]; then
    echo "   FAIL: Found hardcoded hex colors in QML (use DesignTokens instead):"
    echo "$QML_HEX_COLOR_ISSUES"
    ERRORS=$((ERRORS + 1))
else
    echo "   PASS"
fi

# --- Check 4: QML hardcoded named colors ---
echo "4. Checking QML hardcoded named colors (excluding transparent and DesignTokens.qml)..."
QML_NAMED_COLOR_ISSUES=""
while IFS= read -r line; do
    FILE=$(echo "$line" | cut -d: -f1)
    # Skip DesignTokens.qml and AppShadow.qml (whitelisted)
    if [[ "$FILE" == *"DesignTokens.qml" ]] || [[ "$FILE" == *"AppShadow.qml" ]]; then continue; fi
    # Skip "transparent" — structural, not a color value
    if [[ "$line" == *"\"transparent\""* ]]; then continue; fi
    QML_NAMED_COLOR_ISSUES="${QML_NAMED_COLOR_ISSUES}  $line"$'\n'
done < <(cd "$REPO_ROOT" && grep -rn 'color: "white"\|color: "black"\|color: "red"\|color: "green"\|color: "blue"\|color: "gray"\|color: "grey"' apps/desktop/qml/ 2>/dev/null || true)

if [[ -n "$QML_NAMED_COLOR_ISSUES" ]]; then
    echo "   FAIL: Found hardcoded named colors in QML (use DesignTokens instead):"
    echo "$QML_NAMED_COLOR_ISSUES"
    ERRORS=$((ERRORS + 1))
else
    echo "   PASS"
fi

# --- Check 5: Android hardcoded FAB bottom margin (XML block-level) ---
echo "5. Checking Android hardcoded FAB bottom margin (XML block-level)..."
FAB_MARGIN_ISSUES=""

# Find all layout XML files containing FloatingActionButton
while IFS= read -r xmlfile; do
    # Check if any FloatingActionButton element (may span multiple lines) contains layout_margin
    # Using perl -0777 to slurp entire file, then check for FAB block with layout_margin inside
    # [^>]* matches everything except '>' which works because '>' doesn't appear in XML attribute values
    fab_has_margin=$(perl -0777 -ne '
        # Match FAB blocks that contain layout_margin or layout_marginBottom
        # but NOT layout_marginStart/End/Top (horizontal margins are acceptable)
        if (/<FloatingActionButton[^>]*(?:android:layout_margin(?:Bottom)?=)[^>]*\/>/s) { print "yes"; }
        elsif (/<com\.google\.android\.material\.floatingactionbutton\.FloatingActionButton[^>]*(?:android:layout_margin(?:Bottom)?=)[^>]*\/>/s) { print "yes"; }
    ' "$xmlfile" 2>/dev/null || true)

    if [[ "$fab_has_margin" == "yes" ]]; then
        # Exclude FabPlacementHelper references
        file_content=$(perl -0777 -ne 'print' "$xmlfile" 2>/dev/null || true)
        if [[ "$file_content" != *"FabPlacementHelper"* ]]; then
            FAB_MARGIN_ISSUES="${FAB_MARGIN_ISSUES}  $xmlfile: FloatingActionButton with layout_margin"$'\n'
        fi
    fi
done < <(cd "$REPO_ROOT" && find apps/android/app/src/main/res/layout/ -name '*.xml' -type f 2>/dev/null || true)

if [[ -n "$FAB_MARGIN_ISSUES" ]]; then
    echo "   FAIL: Found FloatingActionButton with layout_margin in XML (should use FabPlacementHelper):"
    echo "$FAB_MARGIN_ISSUES"
    ERRORS=$((ERRORS + 1))
else
    echo "   PASS"
fi

# --- Check 6: monetColor expansion ---
echo "6. Checking monetColor is not used in new files..."
MONET_ISSUES=""
while IFS= read -r line; do
    FILE=$(echo "$line" | cut -d: -f1)
    # Skip known legacy files
    if [[ "$FILE" == *"Models.kt" ]] || [[ "$FILE" == *"BridgeMappers.kt" ]] || [[ "$FILE" == *"MainActivity.kt" ]] || [[ "$FILE" == *"SettingsRepository.kt" ]]; then continue; fi
    if [[ "$FILE" == *"settings.rs" ]] || [[ "$FILE" == *"settings_ops.rs" ]] || [[ "$FILE" == *"api.udl" ]]; then continue; fi
    if [[ "$FILE" == *"DesignTokens.qml" ]] || [[ "$FILE" == *"main.qml" ]]; then continue; fi
    if [[ "$FILE" == *"CoreDtos.ets" ]] || [[ "$FILE" == *"MockWriterCoreBridge.ets" ]]; then continue; fi
    if [[ "$FILE" == *"HarmonyThemeAdapter.ets" ]]; then continue; fi
    # Skip Rust backend bridge files (must retain monet_color for backward compat)
    if [[ "$FILE" == *"app_backend.rs" ]] || [[ "$FILE" == *"settings_backend.rs" ]]; then continue; fi
    # Skip core settings definition (already marked deprecated)
    if [[ "$FILE" == *"settings/mod.rs" ]]; then continue; fi
    MONET_ISSUES="${MONET_ISSUES}  $line"$'\n'
done < <(cd "$REPO_ROOT" && grep -rn 'monetColor\|monet_color' apps/ core/ 2>/dev/null | grep -v 'deprecated\|Deprecated\|legacy\|Legacy\|compat\|backward' || true)

if [[ -n "$MONET_ISSUES" ]]; then
    echo "   WARN: monetColor found in non-legacy files (should use theme_palette instead):"
    echo "$MONET_ISSUES"
else
    echo "   PASS"
fi

# --- Self-test (optional) ---
if [[ "${CHECK_UI_TOKENS_SELFTEST:-0}" == "1" ]]; then
    echo ""
    echo "=== Self-test: multi-line XML FAB + layout_margin detection ==="
    SELFTEST_DIR=$(mktemp -d)

    # Test 1: FAB with layout_margin (should be detected)
    SELFTEST_FILE1="$SELFTEST_DIR/test_fab_margin.xml"
    cat > "$SELFTEST_FILE1" << 'XMLEOF'
<?xml version="1.0" encoding="utf-8"?>
<LinearLayout xmlns:android="http://schemas.android.com/apk/res/android"
    android:layout_width="match_parent"
    android:layout_height="match_parent">
    <com.google.android.material.floatingactionbutton.FloatingActionButton
        android:id="@+id/fabTest"
        android:layout_width="wrap_content"
        android:layout_height="wrap_content"
        android:layout_margin="16dp"
        android:contentDescription="test" />
</LinearLayout>
XMLEOF

    fab_has_margin=$(perl -0777 -ne '
        if (/<com\.google\.android\.material\.floatingactionbutton\.FloatingActionButton[^>]*(?:android:layout_margin(?:Bottom)?=)[^>]*\/>/s) { print "yes"; }
    ' "$SELFTEST_FILE1" 2>/dev/null || true)

    if [[ "$fab_has_margin" == "yes" ]]; then
        echo "   SELFTEST 1 PASS: Detected multi-line FAB + layout_margin"
    else
        echo "   SELFTEST 1 FAIL: Did NOT detect multi-line FAB + layout_margin"
        ERRORS=$((ERRORS + 1))
    fi

    # Test 2: FAB with only layout_marginStart/End (should NOT be detected)
    SELFTEST_FILE2="$SELFTEST_DIR/test_fab_margin_start.xml"
    cat > "$SELFTEST_FILE2" << 'XMLEOF'
<?xml version="1.0" encoding="utf-8"?>
<LinearLayout xmlns:android="http://schemas.android.com/apk/res/android"
    android:layout_width="match_parent"
    android:layout_height="match_parent">
    <com.google.android.material.floatingactionbutton.FloatingActionButton
        android:id="@+id/fabTest2"
        android:layout_width="wrap_content"
        android:layout_height="wrap_content"
        android:layout_marginStart="16dp"
        android:layout_marginEnd="16dp"
        android:contentDescription="test" />
</LinearLayout>
XMLEOF

    fab_has_margin2=$(perl -0777 -ne '
        if (/<com\.google\.android\.material\.floatingactionbutton\.FloatingActionButton[^>]*(?:android:layout_margin(?:Bottom)?=)[^>]*\/>/s) { print "yes"; }
    ' "$SELFTEST_FILE2" 2>/dev/null || true)

    if [[ "$fab_has_margin2" == "yes" ]]; then
        echo "   SELFTEST 2 FAIL: FAB with only layout_marginStart/End should NOT be flagged"
        ERRORS=$((ERRORS + 1))
    else
        echo "   SELFTEST 2 PASS: FAB with only layout_marginStart/End correctly not flagged"
    fi

    rm -rf "$SELFTEST_DIR"
fi

echo ""
if [[ $ERRORS -gt 0 ]]; then
    echo "=== UI Token Check FAILED ($ERRORS errors) ==="
    exit 1
else
    echo "=== UI Token Check PASSED ==="
    exit 0
fi
