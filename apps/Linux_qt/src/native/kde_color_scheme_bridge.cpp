// =============================================================================
// kde_color_scheme_bridge.cpp — Linux_qt KDE 应用配色方案桥实现
// =============================================================================
//
// 依赖 KF6::ColorScheme（KColorSchemeManager / KColorSchemeModel）。
// 只切换 KDE 应用配色方案（影响 Kirigami.Theme / Breeze 控件颜色），不改正文
// 配色，也不触碰编辑画布渲染路径。
// =============================================================================

#include "kde_color_scheme_bridge.h"

#include <QAbstractItemModel>
#include <QCoreApplication>
#include <QModelIndex>
#include <QString>
#include <QtGlobal>

#include <KColorSchemeManager>
#include <KColorSchemeModel>

namespace {

// 官方安装的 Breeze 深/浅方案 id。真实运行时仍以 indexForSchemeId 结果为准，
// 若解析不到再退回扫描 model() 的 IdRole。
constexpr const char *kDarkSchemeId = "BreezeDark";
constexpr const char *kLightSchemeId = "BreezeLight";

// 在 manager 的 model() 中按 IdRole 找到包含关键字（dark/light）的方案。
// 仅作为 indexForSchemeId 失败时的兜底，避免把无效 index 当成 dark/light。
QModelIndex findSchemeIdContaining(KColorSchemeManager *manager, const QString &keyword)
{
    QAbstractItemModel *model = manager->model();
    if (model == nullptr) {
        return {};
    }
    const int rows = model->rowCount();
    for (int row = 0; row < rows; ++row) {
        const QModelIndex index = model->index(row, 0);
        const QString id = index.data(KColorSchemeModel::IdRole).toString();
        if (!id.isEmpty() && id.contains(keyword, Qt::CaseInsensitive)) {
            return index;
        }
    }
    return {};
}

// 解析指定深浅的方案 index，失败时返回无效 index。
QModelIndex resolveSchemeIndex(KColorSchemeManager *manager, const QString &preferredId, const QString &keyword)
{
    const QModelIndex direct = manager->indexForSchemeId(preferredId);
    if (direct.isValid()) {
        return direct;
    }
    return findSchemeIdContaining(manager, keyword);
}

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

    const QString requested = QString::fromUtf8(mode).trimmed().toLower();

    if (requested == QLatin1String("system")) {
        // 空 index 在 KColorSchemeManager API 中表示回到系统方案，语义正确。
        manager->activateScheme(QModelIndex());
        return 0;
    }

    QModelIndex target;
    if (requested == QLatin1String("dark")) {
        target = resolveSchemeIndex(manager, QString::fromLatin1(kDarkSchemeId), QStringLiteral("dark"));
    } else if (requested == QLatin1String("light")) {
        target = resolveSchemeIndex(manager, QString::fromLatin1(kLightSchemeId), QStringLiteral("light"));
    } else {
        qWarning("sujian_kde_color_scheme_apply: 未知主题模式 %s", mode);
        return 1;
    }

    if (!target.isValid()) {
        // 无效 index 会被理解为「恢复系统」，绝不能当作 dark/light 传入。
        qWarning("sujian_kde_color_scheme_apply: 找不到 %s 方案，保持现有配色", mode);
        return 1;
    }

    manager->activateScheme(target);
    return 0;
}
