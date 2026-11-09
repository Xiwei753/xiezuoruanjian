// Windows 深浅色应用桥。
//
// 与 Linux 的 KDE 配色桥保持同一个 C 符号和返回值约定，Rust 侧
// native/kde_color_scheme.rs 无需分叉。只切换应用调色板，不改正文渲染。

#include <QApplication>
#include <QColor>
#include <QGuiApplication>
#include <QPalette>
#include <QSettings>
#include <QString>
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

void applyPalette(bool dark)
{
    QPalette palette;
    if (dark) {
        palette.setColor(QPalette::Window, QColor(32, 32, 32));
        palette.setColor(QPalette::WindowText, QColor(238, 238, 238));
        palette.setColor(QPalette::Base, QColor(24, 24, 24));
        palette.setColor(QPalette::AlternateBase, QColor(40, 40, 40));
        palette.setColor(QPalette::Text, QColor(238, 238, 238));
        palette.setColor(QPalette::Button, QColor(45, 45, 45));
        palette.setColor(QPalette::ButtonText, QColor(238, 238, 238));
        palette.setColor(QPalette::Highlight, QColor(0, 120, 212));
        palette.setColor(QPalette::HighlightedText, QColor(255, 255, 255));
        palette.setColor(QPalette::ToolTipBase, QColor(45, 45, 45));
        palette.setColor(QPalette::ToolTipText, QColor(238, 238, 238));
        palette.setColor(QPalette::PlaceholderText, QColor(160, 160, 160));
        palette.setColor(QPalette::Disabled, QPalette::Text, QColor(130, 130, 130));
        palette.setColor(QPalette::Disabled, QPalette::ButtonText, QColor(130, 130, 130));
    } else {
        palette.setColor(QPalette::Window, QColor(243, 243, 243));
        palette.setColor(QPalette::WindowText, QColor(32, 32, 32));
        palette.setColor(QPalette::Base, QColor(255, 255, 255));
        palette.setColor(QPalette::AlternateBase, QColor(247, 247, 247));
        palette.setColor(QPalette::Text, QColor(32, 32, 32));
        palette.setColor(QPalette::Button, QColor(251, 251, 251));
        palette.setColor(QPalette::ButtonText, QColor(32, 32, 32));
        palette.setColor(QPalette::Highlight, QColor(0, 120, 212));
        palette.setColor(QPalette::HighlightedText, QColor(255, 255, 255));
        palette.setColor(QPalette::ToolTipBase, QColor(255, 255, 255));
        palette.setColor(QPalette::ToolTipText, QColor(32, 32, 32));
        palette.setColor(QPalette::PlaceholderText, QColor(120, 120, 120));
        palette.setColor(QPalette::Disabled, QPalette::Text, QColor(160, 160, 160));
        palette.setColor(QPalette::Disabled, QPalette::ButtonText, QColor(160, 160, 160));
    }
    QApplication::setPalette(palette);
}

} // namespace

extern "C" int sujian_kde_color_scheme_apply(const char *mode)
{
    if (mode == nullptr || QApplication::instance() == nullptr) {
        return 1;
    }

    const QString requested = QString::fromUtf8(mode).trimmed().toLower();
    const bool system = requested == QLatin1String("system");
    bool dark = false;
    if (system) {
        dark = systemPrefersDark();
    } else if (requested == QLatin1String("dark")) {
        dark = true;
    } else if (requested == QLatin1String("light")) {
        dark = false;
    } else {
        return 1;
    }

#if QT_VERSION >= QT_VERSION_CHECK(6, 8, 0)
    if (system) {
        // Qt 6.8+ 显式 Dark/Light 会覆盖系统颜色方案；system 必须回到 Unknown，
        // 让 Windows/Qt 自己推送系统主题变化。
        QGuiApplication::styleHints()->setColorScheme(Qt::ColorScheme::Unknown);
        return 0;
    }
    QGuiApplication::styleHints()->setColorScheme(
        dark ? Qt::ColorScheme::Dark : Qt::ColorScheme::Light);
    // setColorScheme 可能触发 Qt 重新套用默认调色板；随后写入应用自定义调色板，
    // 保证显式浅色/深色选择最终生效。
    applyPalette(dark);
#else
    // Qt 6.7 没有 QStyleHints::setColorScheme，只能用调色板表达当前选择。
    applyPalette(dark);
#endif
    return 0;
}
