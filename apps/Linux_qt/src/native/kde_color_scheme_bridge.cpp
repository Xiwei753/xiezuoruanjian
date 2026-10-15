// =============================================================================
// kde_color_scheme_bridge.cpp — Linux_qt KDE 应用配色方案桥实现
// =============================================================================
//
// 依赖 KF6::ColorScheme（KColorSchemeManager）。
// 只切换 KDE 应用配色方案（影响 Kirigami.Theme / Breeze 控件颜色），不改正文
// 配色，也不触碰编辑画布渲染路径。
//
// 持久化说明：Core 的 appearance_mode 是唯一事实来源，本桥只执行「当次应用」，
// 因此必须关闭 KColorSchemeManager 默认开启的 setAutosaveChanges，避免 KDE 额外
// 记一份会抢先恢复/覆盖 Core 的方案选择。
// =============================================================================

#include "kde_color_scheme_bridge.h"

#include <QCoreApplication>
#include <QModelIndex>
#include <QString>
#include <QtGlobal>

#include <KColorSchemeManager>

namespace {

// Breeze 深/浅方案的固定 id。plasma-breeze-common 提供对应的
// /usr/share/color-schemes/BreezeDark.colors 与 BreezeLight.colors。
// 不使用模糊匹配：用户选择的是 Breeze，缺失时明确报错，绝不套用其他主题，
// 也绝不把无效 index 传给 activateScheme（无效 index 表示恢复系统）。
constexpr const char *kDarkSchemeId = "BreezeDark";
constexpr const char *kLightSchemeId = "BreezeLight";

} // namespace

extern "C" int sujian_kde_color_scheme_apply(const char *mode)
{
    if (mode == nullptr || QCoreApplication::instance() == nullptr) {
        return 1;
    }

    KColorSchemeManager *manager = KColorSchemeManager::instance();
    if (manager == nullptr) {
        qWarning("sujian_kde_color_scheme_apply: KColorSchemeManager 不可用");
        return 1;
    }

    // Core appearance_mode 是唯一持久化源；KDE manager 只执行当次应用。
    manager->setAutosaveChanges(false);

    const QString requested = QString::fromUtf8(mode).trimmed().toLower();

    if (requested == QLatin1String("system")) {
        // 空 index 在 KColorSchemeManager API 中表示回到系统方案，语义正确。
        manager->activateScheme(QModelIndex());
        return 0;
    }

    const char *schemeId = nullptr;
    if (requested == QLatin1String("dark")) {
        schemeId = kDarkSchemeId;
    } else if (requested == QLatin1String("light")) {
        schemeId = kLightSchemeId;
    } else {
        qWarning("sujian_kde_color_scheme_apply: 未知主题模式 %s", mode);
        return 1;
    }

    const QModelIndex target = manager->indexForSchemeId(QString::fromLatin1(schemeId));
    if (!target.isValid()) {
        qWarning(
            "sujian_kde_color_scheme_apply: 找不到 %s 方案（需安装 plasma-breeze-common），保持现有配色",
            schemeId);
        return 1;
    }

    manager->activateScheme(target);
    return 0;
}
