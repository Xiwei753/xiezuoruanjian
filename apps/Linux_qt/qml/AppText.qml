import QtQuick
import QtQuick.Controls

Text {
    id: control
    required property var dt
    // Issue #701 评论 5699565102: 删除内部 fallbackDt。组件必须消费调用方
    // 传入的根 dt；漏传就是调用错误，不偷偷生成独立主题。
    // Issue #715: 改为 required property，不再允许组件先以 null 创建。
    readonly property var resolvedDt: dt
    property string variant: "primary"

    color: {
        switch (control.variant) {
            case "secondary": return resolvedDt.textSecondary;
            case "muted": return resolvedDt.textMuted;
            case "disabled": return resolvedDt.textDisabled;
            case "onPrimary": return resolvedDt.onPrimary;
            case "selected": return resolvedDt.selectedText;
            case "onSurface": return resolvedDt.onSurface;
            case "onSurfaceVariant": return resolvedDt.onSurfaceVariant;
            case "onPrimaryContainer": return resolvedDt.onPrimaryContainer;
            case "onSecondaryContainer": return resolvedDt.onSecondaryContainer;
            case "onError": return resolvedDt.onError;
            case "onDangerContainer": return resolvedDt.onDangerContainer;
            case "primary":
            default:
                return resolvedDt.textPrimary;
        }
    }
    font.pointSize: resolvedDt.fontMdPt
    wrapMode: Text.WordWrap
}
