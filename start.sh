#!/usr/bin/env bash
set -euo pipefail

# Check arguments
if [ $# -gt 0 ]; then
    if [ "$1" = "debug" ]; then
        if [ ! -f "./start-debug.sh" ]; then
            echo "Error: start-debug.sh not found." >&2
            exit 1
        fi
        shift
        exec ./start-debug.sh "$@"
    else
        echo "Error: Unknown argument '$1'" >&2
        echo "Usage:" >&2
        echo "  bash start" >&2
        echo "  bash start debug [sync|tree|ui|all|trace|qt]" >&2
        exit 1
    fi
fi

export QT_QUICK_CONTROLS_STYLE=Basic

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

prepend_path_var() {
    local var_name="$1"
    local path_value="$2"
    local current_value="${!var_name:-}"
    case ":$current_value:" in
        *":$path_value:"*) ;;
        "::") export "$var_name=$path_value" ;;
        *) export "$var_name=$path_value:$current_value" ;;
    esac
}

detect_qt6_header_version() {
    local header=""
    for h in "/run/host/usr/include/qt6/QtCore/qtcoreversion.h" "/usr/include/qt6/QtCore/qtcoreversion.h"; do
        [ -f "$h" ] && header="$h" && break
    done
    [ -n "$header" ] || return 1
    local line version
    while IFS= read -r line; do
        case "$line" in
            *QTCORE_VERSION_STR*)
                version="${line#*\"}"
                version="${version%%\"*}"
                [[ "$version" == 6.* ]] || return 1
                printf '%s\n' "$version"
                return 0
                ;;
        esac
    done < "$header"
    return 1
}

detect_qt6_qmake() {
    local candidates=(
        "${QMAKE:-}"
        "/run/host/usr/lib64/qt6/bin/qmake"
        "/run/host/usr/lib64/qt6/bin/qmake6"
        "/run/host/usr/bin/qmake6"
        "/run/host/usr/bin/qmake-qt6"
        "/usr/lib64/qt6/bin/qmake"
        "/usr/lib64/qt6/bin/qmake6"
        "/usr/bin/qmake6"
        "/usr/bin/qmake-qt6"
        "qmake6"
        "qmake-qt6"
    )
    local candidate version
    for candidate in "${candidates[@]}"; do
        [ -n "$candidate" ] || continue
        local ld_path=""
        case "$candidate" in
            /run/host/*) ld_path="LD_LIBRARY_PATH=/run/host/usr/lib64" ;;
        esac
        if version="$(${ld_path:+$ld_path }$candidate -query QT_VERSION 2>/dev/null)" && [[ "$version" == 6.* ]]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done
    return 1
}

if QMAKE_DETECTED="$(detect_qt6_qmake)"; then
    export QMAKE="$QMAKE_DETECTED"
    export QT_VERSION_MAJOR=6
    local_ld_path=""
    case "$QMAKE" in
        /run/host/*) local_ld_path="LD_LIBRARY_PATH=/run/host/usr/lib64" ;;
    esac
    QT_VERSION_DETECTED="$(${local_ld_path:+$local_ld_path }$QMAKE -query QT_VERSION 2>/dev/null || true)"
elif QT_VERSION_DETECTED="$(detect_qt6_header_version)"; then
    if [ -d "/run/host/usr/include/qt6" ]; then
        export QT_INCLUDE_PATH="${QT_INCLUDE_PATH:-/run/host/usr/include/qt6}"
        export QT_LIBRARY_PATH="${QT_LIBRARY_PATH:-/run/host/usr/lib64}"
    else
        export QT_INCLUDE_PATH="${QT_INCLUDE_PATH:-/usr/include/qt6}"
        export QT_LIBRARY_PATH="${QT_LIBRARY_PATH:-/usr/lib64}"
    fi
    export QT_VERSION_MAJOR=6
else
    QT_VERSION_DETECTED="unknown"
fi


print_desktop_runtime_profile() {
    local label="$1"
    local qml_import_path="${QML2_IMPORT_PATH:-${QML_IMPORT_PATH:-<unset>}}"
    local qt_plugin_path="${QT_PLUGIN_PATH:-<unset>}"
    local platform_name="${QT_QPA_PLATFORM:-$( [ -n "${WAYLAND_DISPLAY:-}" ] && echo wayland || echo xcb )}"
    local input_method_module="${QT_IM_MODULE:-${QT_IM_MODULES:-<unset>}}"
    local bundled_qt="false"
    local runtime_profile="linux-debug"
    if [ -n "${APPIMAGE:-}" ]; then
        bundled_qt="true"
        runtime_profile="linux-appimage"
    fi
    echo "[$label] DesktopRuntimeProfile runtimeProfile=$runtime_profile qtRuntimeVersion=${QT_VERSION_DETECTED:-unknown} qtBuildVersion=${QT_VERSION_DETECTED:-unknown} qmlEntry=qrc:/main.qml qmlImportPath=$qml_import_path qtPluginPath=$qt_plugin_path qrcRevision=${SUJIAN_QRC_REVISION:-package:qml_resources_v1} platformName=$platform_name inputMethodModule=$input_method_module bundledQt=$bundled_qt"
}

# Determine Qt6 QML/plugin paths
if [ -d "/run/host/usr/lib64/qt6/qml" ]; then
    prepend_path_var QML2_IMPORT_PATH "/run/host/usr/lib64/qt6/qml"
    prepend_path_var QML_IMPORT_PATH "/run/host/usr/lib64/qt6/qml"
    prepend_path_var QT_PLUGIN_PATH "/run/host/usr/lib64/qt6/plugins"
else
    prepend_path_var QML2_IMPORT_PATH "/usr/lib64/qt6/qml"
    prepend_path_var QML_IMPORT_PATH "/usr/lib64/qt6/qml"
    prepend_path_var QT_PLUGIN_PATH "/usr/lib64/qt6/plugins"
fi

# Ensure LD_LIBRARY_PATH includes Qt6 library path for runtime
if [ -d "/run/host/usr/lib64" ]; then
    prepend_path_var LD_LIBRARY_PATH "/run/host/usr/lib64"
fi

echo "[start] Qt version detected: $QT_VERSION_DETECTED"
echo "[start] Qt C++ standard: -std=c++17"
echo "[start] QMAKE: ${QMAKE:-not found}"
echo "[start] QT_INCLUDE_PATH: ${QT_INCLUDE_PATH:-}"
echo "[start] QT_LIBRARY_PATH: ${QT_LIBRARY_PATH:-}"
echo "[start] QML2_IMPORT_PATH: ${QML2_IMPORT_PATH:-}"
echo "[start] QT_PLUGIN_PATH: ${QT_PLUGIN_PATH:-}"
print_desktop_runtime_profile "start"
echo "[start] QtQuick.Window qmldir: $( [ -f /run/host/usr/lib64/qt6/qml/QtQuick/Window/qmldir ] && echo found || ( [ -f /usr/lib64/qt6/qml/QtQuick/Window/qmldir ] && echo found || echo missing ) )"
echo "[start] QtQuick Controls qmldir: $( [ -f /run/host/usr/lib64/qt6/qml/QtQuick/Controls/qmldir ] && echo found || ( [ -f /usr/lib64/qt6/qml/QtQuick/Controls/qmldir ] && echo found || echo missing ) )"

echo "[start] Building sujian-desktop package..."
cargo build -p sujian-desktop

echo "[start] Running 素笺写作..."
cargo run -p sujian-desktop
