#!/bin/bash
# AppImage entry point. linuxdeploy's Qt hook is sourced first, then fallback
# directories cover deployments where the hook is absent. The common launcher
# remains the single entry point for RPM and AppImage.
set -euo pipefail

HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
export APPDIR="${APPDIR:-$HERE}"

# Issue #1014：外部 AI CLI 以 `<AppImage> --sujian-mcp ...` 启动。
# 必须在 Qt hooks / GUI launcher / 启动日志镜像之前分流：stdout 从进程启动起
# 只承载 MCP JSON-RPC，诊断信息由 MCP 自己写 stderr。
if [ "${1:-}" = "--sujian-mcp" ]; then
    shift
    for libdir in \
        "$APPDIR/usr/lib" \
        "$APPDIR/usr/lib64" \
        "$APPDIR/usr/lib/x86_64-linux-gnu" \
        "$APPDIR/usr/lib/aarch64-linux-gnu"; do
        [ -d "$libdir" ] || continue
        case ":${LD_LIBRARY_PATH:-}:" in
            *":$libdir:"*) ;;
            *) export LD_LIBRARY_PATH="$libdir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" ;;
        esac
    done
    exec "$APPDIR/usr/libexec/sujian/sujian_mcp" "$@"
fi

if [ -d "$APPDIR/apprun-hooks" ]; then
    for hook in "$APPDIR"/apprun-hooks/*.sh; do
        [ -f "$hook" ] || continue
        # shellcheck source=/dev/null
        . "$hook"
    done
fi

for libdir in \
    "$APPDIR/usr/lib" \
    "$APPDIR/usr/lib64" \
    "$APPDIR/usr/lib/x86_64-linux-gnu" \
    "$APPDIR/usr/lib/aarch64-linux-gnu"; do
    [ -d "$libdir" ] || continue
    case ":${LD_LIBRARY_PATH:-}:" in
        *":$libdir:"*) ;;
        *) export LD_LIBRARY_PATH="$libdir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" ;;
    esac
done

# 系统 bwrap 可能是 setuid 安装，优先使用；否则回退到 AppImage 内置副本。
if [ -z "${SUJIAN_BWRAP:-}" ] && ! command -v bwrap >/dev/null 2>&1; then
    export SUJIAN_BWRAP="$APPDIR/usr/libexec/sujian/bwrap"
fi

if [ -z "${QT_PLUGIN_PATH:-}" ]; then
    export QT_PLUGIN_PATH="$APPDIR/usr/plugins"
fi
if [ -z "${QML2_IMPORT_PATH:-}" ]; then
    export QML2_IMPORT_PATH="$APPDIR/usr/qml"
fi
if [ -z "${QML_IMPORT_PATH:-}" ]; then
    export QML_IMPORT_PATH="$QML2_IMPORT_PATH"
fi
if [ -z "${QT_QPA_PLATFORM_PLUGIN_PATH:-}" ]; then
    export QT_QPA_PLATFORM_PLUGIN_PATH="$QT_PLUGIN_PATH/platforms"
fi

# AppDir places the ELF in usr/bin while RPM uses usr/libexec. The common
# launcher receives the actual path explicitly.
export SUJIAN_REAL_BINARY="$APPDIR/usr/bin/sujian-linux-qt"
exec "$APPDIR/usr/libexec/sujian/sujian-launcher.sh" "$@"
