// Windows 深浅色应用桥。
//
// 与 Linux 的 KDE 配色桥保持同一个 C 符号和返回值约定，Rust 侧
// native/kde_color_scheme.rs 无需分叉。Windows 不手写视觉 token：显式
// 深浅色由 Qt 颜色方案（Qt/Breeze/Kirigami 主题链）推导，这里只做切换。
// 另外为便携包注册随包 Breeze 图标主题的搜索路径，供 Kirigami.Icon 与
// KIconThemes 在无系统 KDE 安装的 Windows 机器上找到真实图标。

#include <breezeicons.h>

#include <QApplication>
#include <QCoreApplication>
#include <QGuiApplication>
#include <QIcon>
#include <QPalette>
#include <QSettings>
#include <QString>
#include <QStringList>
#include <QStyleHints>
#include <QtGlobal>

namespace {

bool systemPrefersDark()
{
    QSettings settings(
        QStringLiteral("HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
        QSettings::NativeFormat);
    // AppsUseLightTheme: 1 = 浅色，0 = 深色；缺失时默认浅色。
    return settings.value(QStringLiteral("AppsUseLightTheme"), 1).toInt() == 0;
}

} // namespace

extern "C" int sujian_kde_color_scheme_apply(const char *mode)
{
    if (mode == nullptr || QApplication::instance() == nullptr) {
        return 1;
    }

    const QString requested = QString::fromUtf8(mode).trimmed().toLower();
    if (requested == QLatin1String("system")) {
        // 先清除显式设置过的调色板角色，否则 qt 只会刷新系统默认 palette 条目，
        // 手动深色留下的 QPalette 会继续生效。再回到 Unknown，让 Qt/Breeze
        // 自己按 Windows 深浅色推送系统主题变化。
        QGuiApplication::setPalette(QPalette());
        QGuiApplication::styleHints()->setColorScheme(Qt::ColorScheme::Unknown);
        return 0;
    }

    bool dark = false;
    if (requested == QLatin1String("dark")) {
        dark = true;
    } else if (requested != QLatin1String("light")) {
        return 1;
    }

    // 配色不再手写 Windows 独立调色板：颜色由 Qt 颜色方案驱动，控件、
    // Kirigami Theme 与共享 QML 主题取同一来源。
    QGuiApplication::styleHints()->setColorScheme(
        dark ? Qt::ColorScheme::Dark : Qt::ColorScheme::Light);
    return 0;
}

extern "C" void sujian_windows_icon_theme_init(const char *app_dir)
{
    if (app_dir == nullptr) {
        return;
    }
    // KDE's Windows Craft package embeds Breeze in KF6BreezeIcons.dll.
    // initIcons registers the Qt resource tree at :/icons/breeze.
    BreezeIcons::initIcons();

    QStringList paths = QIcon::themeSearchPaths();
    if (!paths.contains(QStringLiteral(":/icons"))) {
        paths.prepend(QStringLiteral(":/icons"));
    }
    QIcon::setThemeSearchPaths(paths);
    QIcon::setThemeName(QStringLiteral("breeze"));
    QIcon::setFallbackThemeName(QStringLiteral("breeze"));
}
