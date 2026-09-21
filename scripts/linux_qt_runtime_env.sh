#!/usr/bin/env bash
# scripts/linux_qt_runtime_env.sh — Issue #729 评论 5762596831 第 1 部分
#
# 共享的 Linux Qt 运行环境收口逻辑：统一 Wayland QPA / 输入法检测设置。
# 被 start.sh 和 start-debug.sh source，不直接执行。
#
# source 后调用 sujian_configure_wayland_im_env [label] 即可。
# label 可选，用于日志前缀（如 "start" / "start-debug"）。
#
# 兼容 set -euo pipefail：本脚本只定义函数，不执行副作用；函数内部对可选
# 命令失败做容错（用 if 包裹，不触发 set -e 退出），用 return 而非 exit。

# 防止重复 source 自身
if [ -n "${_SUJIAN_RUNTIME_ENV_SOURCED:-}" ]; then
    return 0 2>/dev/null || true
fi
_SUJIAN_RUNTIME_ENV_SOURCED=1

# 路径前置辅助函数：把 path_value 前置到 var_name 指定的环境变量，
# 已存在则不重复添加。与 start.sh / start-debug.sh 的 prepend_path_var
# 等价但独立命名，避免与调用脚本的同名函数冲突。
sujian_prepend_path_var() {
    local var_name="$1"
    local path_value="$2"
    local current_value="${!var_name:-}"
    case ":$current_value:" in
        *":$path_value:"*) ;;
        "::") export "$var_name=$path_value" ;;
        *) export "$var_name=$path_value:$current_value" ;;
    esac
}

# 检测 Wayland 会话：XDG_SESSION_TYPE=wayland 或 WAYLAND_DISPLAY 非空
sujian_is_wayland_session() {
    if [ "${XDG_SESSION_TYPE:-}" = "wayland" ]; then
        return 0
    fi
    if [ -n "${WAYLAND_DISPLAY:-}" ]; then
        return 0
    fi
    return 1
}

# 检测指定进程是否在运行（pgrep -x）。pgrep 不存在或未运行均返回非零。
# 用 if 包裹避免 set -e 退出。
sujian_process_running() {
    local name="$1"
    if ! command -v pgrep &>/dev/null; then
        return 1
    fi
    if pgrep -x "$name" &>/dev/null; then
        return 0
    fi
    return 1
}

# 检测 fcitx5 是否可用（命令存在或进程在运行）
sujian_fcitx5_available() {
    if command -v fcitx5 &>/dev/null; then
        return 0
    fi
    sujian_process_running "fcitx5"
}

# 检测 ibus 是否可用（命令存在或进程在运行）
sujian_ibus_available() {
    if command -v ibus-daemon &>/dev/null; then
        return 0
    fi
    sujian_process_running "ibus-daemon"
}

# 确保 fcitx5 Qt6 plugin path 可发现（前置 platforminputcontexts 的父目录到
# QT_PLUGIN_PATH）。找不到任一候选目录时静默返回。
sujian_ensure_fcitx5_qt6_plugin_path() {
    local fcitx_plugin_dir
    for fcitx_plugin_dir in \
        "/usr/lib64/qt6/plugins/platforminputcontexts" \
        "/usr/lib/x86_64-linux-gnu/qt6/plugins/platforminputcontexts" \
        "/usr/lib/qt6/plugins/platforminputcontexts" \
        "/app/usr/lib64/qt6/plugins/platforminputcontexts"; do
        if [ -d "$fcitx_plugin_dir" ]; then
            sujian_prepend_path_var QT_PLUGIN_PATH "$(dirname "$fcitx_plugin_dir")"
            return 0
        fi
    done
    return 0
}

# 主入口：统一配置 Wayland QPA 平台和输入法环境变量。
#
# - 幂等：只在环境变量未设置时补默认，不覆盖用户显式设置。
# - Wayland 会话且 QT_QPA_PLATFORM 未设时，设为 wayland，拒绝默认回退 xcb。
# - 检测 fcitx5/ibus 设 QT_IM_MODULE / QT_IM_MODULES；Wayland 下优先
#   Wayland text-input 协议（QT_IM_MODULES=wayland;fcitx;ibus，wayland 在前）。
# - 确保 fcitx5 Qt6 plugin path 可发现。
sujian_configure_wayland_im_env() {
    local label="${1:-start}"
    local wayland=false
    if sujian_is_wayland_session; then
        wayland=true
    fi

    # QT_QPA_PLATFORM：Wayland 会话且未显式设置时补 wayland，拒绝默认回退 xcb
    if [ -z "${QT_QPA_PLATFORM:-}" ]; then
        if [ "$wayland" = "true" ]; then
            export QT_QPA_PLATFORM=wayland
            echo "[$label] Wayland session detected: QT_QPA_PLATFORM=$QT_QPA_PLATFORM (refusing xcb/XWayland fallback)"
        fi
    else
        if [ "$wayland" = "true" ]; then
            case ":$QT_QPA_PLATFORM:" in
                *":wayland:"*) ;;
                *)
                    echo "[$label] WARNING: Wayland session but QT_QPA_PLATFORM=$QT_QPA_PLATFORM (no wayland); respecting user setting without override" >&2
                    ;;
            esac
        fi
    fi

    # QT_IM_MODULE / QT_IM_MODULES：仅在用户未显式设置任一时补默认
    if [ -z "${QT_IM_MODULE:-}" ] && [ -z "${QT_IM_MODULES:-}" ]; then
        if [ "$wayland" = "true" ]; then
            if sujian_fcitx5_available; then
                export QT_IM_MODULES="wayland;fcitx;ibus"
                echo "[$label] Wayland + fcitx5 detected: QT_IM_MODULES=$QT_IM_MODULES"
            elif sujian_ibus_available; then
                export QT_IM_MODULES="wayland;ibus"
                echo "[$label] Wayland + ibus detected: QT_IM_MODULES=$QT_IM_MODULES"
            else
                echo "[$label] Wayland detected but no IM framework found; relying on Qt Wayland text-input protocol"
            fi
        else
            if sujian_fcitx5_available; then
                export QT_IM_MODULE=fcitx
                echo "[$label] Non-Wayland + fcitx5 detected: QT_IM_MODULE=$QT_IM_MODULE"
            elif sujian_ibus_available; then
                export QT_IM_MODULE=ibus
                echo "[$label] Non-Wayland + ibus detected: QT_IM_MODULE=$QT_IM_MODULE"
            fi
        fi
    fi

    # 确保 fcitx5 Qt6 plugin path 可发现（当 IM 配置含 fcitx 时）
    if { [ -n "${QT_IM_MODULE:-}" ] && [ "$QT_IM_MODULE" = "fcitx" ]; } \
        || { [ -n "${QT_IM_MODULES:-}" ] && echo "$QT_IM_MODULES" | grep -q fcitx; }; then
        sujian_ensure_fcitx5_qt6_plugin_path
    fi
}
