use super::*;
use crate::backend::AppRef;
use crate::backend::DomainSnapshot;
use qmetaobject::QColor;

/// Issue #701 评论 5699565102: 运行时主题状态的唯一事实来源。
/// Issue #701 评论 5702214893: resolved scheme 现在缓存在 controller 内。
///
/// `appearance_mode`、`system_is_dark`、`is_dark`、`color_source`、当前
/// builtin/palette id 以及最终 `ThemeColorSchemeDto`（`scheme`）
/// 全部从同一份 `DomainSnapshot` 一次性解析并缓存到 `cached_state`。
/// getter（`resolved_scheme_json`/`is_dark`/`color_source` 等）只读缓存，
/// 不再每次都 borrow `AppBackend` 或 `DomainSnapshot`，彻底消除双状态机。
///
/// 切换主题模式、颜色来源、内置主题、已保存 palette 后，setter 先写
/// AppBackend 设置，再调用 `rebuild_resolved_state()` 重建缓存，最后发
/// `scheme_changed`；QML 侧通过该信号重新求值只读属性。`with_app` 仅用于
/// 调用 `core_api()` 加载 palette record / builtin theme 数据，不再用于
/// 读取主题设置本身。
///
/// Issue #709 评论 issue-body-709: `ResolvedThemeState` 直接保存最终选中的
/// `ThemeColorSchemeDto`（`scheme: Option<...>`），不再序列化成 JSON 字符串。
/// `theme_state_json()` 直接序列化这一份完整状态，消除
/// scheme -> JSON string -> serde_json::Value -> 再包第二层 JSON 的低效路径。
#[allow(non_snake_case)] // Qt QML naming convention
#[derive(QObject, Default)]
pub struct LinuxThemeController {
    #[allow(dead_code)]
    base: qt_base_class!(trait QObject),
    /// Issue #702: 主题完整状态一次性发布。JSON 结构：
    /// `{"appearance_mode": str, "system_is_dark": bool, "is_dark": bool,
    ///   "color_source": str, "selected_builtin_theme_id": str,
    ///   "selected_palette_id": str, "scheme": <ThemeColorScheme object or null>}`。
    /// QML 侧（DesignTokens）只绑定这一个属性，从同一份 JSON 解析
    /// `is_dark` 和 `scheme`，彻底消除 isDark 与 scheme 不同步的中间状态。
    /// Issue #709 评论 issue-body-709: scheme 为 null 时 QML 侧 fallback 到
    /// isDark 派生的固定色。
    #[allow(dead_code)]
    theme_state_json: qt_property!(QString; READ theme_state_json NOTIFY scheme_changed),
    #[allow(dead_code)]
    resolved_scheme_json: qt_property!(QString; READ resolved_scheme_json NOTIFY scheme_changed),
    #[allow(dead_code)]
    is_dark: qt_property!(bool; READ is_dark NOTIFY scheme_changed),
    #[allow(dead_code)]
    color_source: qt_property!(QString; READ color_source NOTIFY scheme_changed),
    #[allow(dead_code)]
    selected_builtin_theme_id: qt_property!(QString; READ selected_builtin_theme_id NOTIFY scheme_changed),
    #[allow(dead_code)]
    selected_palette_id: qt_property!(QString; READ selected_palette_id NOTIFY scheme_changed),
    #[allow(dead_code)]
    appearance_mode: qt_property!(QString; READ appearance_mode NOTIFY scheme_changed),
    #[allow(dead_code)]
    system_is_dark: qt_property!(bool; READ system_is_dark NOTIFY scheme_changed),
    // Issue #712: QColor 属性直接从 scheme 读取并返回 QColor，
    // 不再经过 theme_state_json -> JSON.parse -> JS 字符串 -> Qt.rgba。
    // JSON 只留给诊断，不再参与真正绘制。
    #[allow(dead_code)]
    primary: qt_property!(QColor; READ primary_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    on_primary: qt_property!(QColor; READ on_primary_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    primary_container: qt_property!(QColor; READ primary_container_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    on_primary_container: qt_property!(QColor; READ on_primary_container_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    secondary: qt_property!(QColor; READ secondary_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    on_secondary: qt_property!(QColor; READ on_secondary_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    secondary_container: qt_property!(QColor; READ secondary_container_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    on_secondary_container: qt_property!(QColor; READ on_secondary_container_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    tertiary: qt_property!(QColor; READ tertiary_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    on_tertiary: qt_property!(QColor; READ on_tertiary_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    tertiary_container: qt_property!(QColor; READ tertiary_container_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    on_tertiary_container: qt_property!(QColor; READ on_tertiary_container_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    background: qt_property!(QColor; READ background_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    on_background: qt_property!(QColor; READ on_background_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    surface: qt_property!(QColor; READ surface_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    on_surface: qt_property!(QColor; READ on_surface_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    surface_variant: qt_property!(QColor; READ surface_variant_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    on_surface_variant: qt_property!(QColor; READ on_surface_variant_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    surface_tint: qt_property!(QColor; READ surface_tint_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    surface_dim: qt_property!(QColor; READ surface_dim_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    surface_bright: qt_property!(QColor; READ surface_bright_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    surface_container_lowest: qt_property!(QColor; READ surface_container_lowest_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    surface_container_low: qt_property!(QColor; READ surface_container_low_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    surface_container: qt_property!(QColor; READ surface_container_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    surface_container_high: qt_property!(QColor; READ surface_container_high_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    surface_container_highest: qt_property!(QColor; READ surface_container_highest_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    inverse_surface: qt_property!(QColor; READ inverse_surface_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    inverse_on_surface: qt_property!(QColor; READ inverse_on_surface_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    inverse_primary: qt_property!(QColor; READ inverse_primary_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    error: qt_property!(QColor; READ error_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    on_error: qt_property!(QColor; READ on_error_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    error_container: qt_property!(QColor; READ error_container_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    on_error_container: qt_property!(QColor; READ on_error_container_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    outline: qt_property!(QColor; READ outline_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    outline_variant: qt_property!(QColor; READ outline_variant_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    scrim: qt_property!(QColor; READ scrim_qcolor NOTIFY scheme_changed),
    #[allow(dead_code)]
    scheme_changed: qt_signal!(),
    #[allow(dead_code)]
    reload: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    set_color_source: qt_method!(fn(&mut self, val: QString)),
    #[allow(dead_code)]
    set_appearance_mode: qt_method!(fn(&mut self, val: QString)),
    #[allow(dead_code)]
    set_selected_builtin_theme_id: qt_method!(fn(&mut self, val: QString)),
    #[allow(dead_code)]
    set_selected_palette_id: qt_method!(fn(&mut self, val: QString)),
    #[allow(dead_code)]
    set_system_is_dark: qt_method!(fn(&mut self, val: bool)),
    app: AppRef,
    /// Issue #701 评论 5702214893: resolved theme state 缓存。
    ///
    /// setter 写完 AppBackend 设置后调用 `rebuild_resolved_state()` 重建此缓存；
    /// getter 只读此缓存，不再每次都 borrow `AppBackend`/`DomainSnapshot`。
    /// 不参与 QML 绑定（QML 只通过 qt_property READ 方法间接读取）。
    cached_state: std::cell::RefCell<Option<ResolvedThemeState>>,
}

impl LinuxThemeController {
    pub fn new(app: AppRef) -> Self {
        Self {
            app,
            ..Default::default()
        }
    }

    fn with_app<R>(&self, f: impl FnOnce(&AppBackend) -> R) -> Result<R, super::AppBorrowError> {
        self.app.with_app(f)
    }

    fn with_app_mut<R>(
        &self,
        f: impl FnOnce(&mut AppBackend) -> R,
    ) -> Result<R, super::AppBorrowError> {
        self.app.with_app_mut(f)
    }

    fn snap(&self) -> std::cell::Ref<'_, DomainSnapshot> {
        self.app.snapshot().borrow()
    }

    /// 从同一份 `DomainSnapshot` 一次性解析当前运行时主题状态，并加载最终
    /// `ThemeColorSchemeDto`。
    ///
    /// 返回完整的 `ResolvedThemeState`（含 `scheme`）。所有 QML 只读属性
    /// （`appearance_mode`、`is_dark`、`color_source`、`resolved_scheme_json`
    /// 等）都基于这同一份状态，避免一个属性读 snapshot、另一个属性临时再
    /// borrow `AppBackend` 造成的双状态机。
    ///
    /// Issue #701 评论 5702214893: 此方法在 setter 写完设置后调用一次，结果
    /// 缓存到 `cached_state`；getter 通过 `state()` 只读缓存。
    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    /// Issue #709 评论 issue-body-709: 直接把 `Option<ThemeColorSchemeDto>` 存进
    /// `ResolvedThemeState.scheme`，不再序列化成 JSON 字符串。scheme 选择按
    /// `is_dark` 只选一次 `dark_scheme/light_scheme`，彻底消除 is_dark 已是
    /// true 但 scheme 还是浅色值的中间状态。
    pub fn rebuild_resolved_state(&self) -> ResolvedThemeState {
        let s = self.snap();
        let appearance_mode = s.appearance_mode.clone();
        let system_is_dark = s.system_is_dark;
        let is_dark = Self::compute_is_dark(&appearance_mode, system_is_dark);
        // Issue #705: 复现阶段注入的 [BUGFIX_REPRO_TRACE] 诊断 eprintln 已移除。
        // 主题状态决策点:appearance_mode + system_is_dark -> is_dark,
        // on_surface/editorText 从同一份 is_dark 派生的 scheme 取值。
        let color_source = s.color_source.clone();
        let selected_palette_id = s.selected_palette_id.clone();
        let selected_builtin_theme_id = s.selected_builtin_theme_id.clone();
        // drop Ref<'_, DomainSnapshot> 后再 with_app 加载 scheme，避免与
        // with_app 的 AppBackend borrow 冲突（snap() 持有的是 snapshot 的 Ref，
        // 与 with_app 的 AppBackend borrow 是不同的 RefCell，但显式 drop 更清晰）。
        drop(s);

        // Issue #709 评论 5728916561: 把 saved_palette 和 builtin 两条路径的
        // scheme 加载与 resolved_source 追踪合并为一个 (Option<scheme>, String) 元组，
        // 避免在 fallback 分支中重复代码。
        let (scheme, resolved_source) = if color_source == "saved_palette" {
            if !selected_palette_id.is_empty() {
                let parts: Vec<&str> = selected_palette_id.splitn(2, ':').collect();
                if parts.len() == 2 {
                    match self
                        .with_app(|app| {
                            app.core_api().and_then(|core| {
                                core.load_palette_record(parts[0], parts[1])
                                    .ok()
                                    .map(|dto| {
                                        let dto: writer_core::api::types::ThemePaletteRecordDto =
                                            dto;
                                        if is_dark {
                                            dto.dark_scheme
                                        } else {
                                            dto.light_scheme
                                        }
                                    })
                            })
                        })
                        .unwrap_or(None)
                    {
                        Some(s) => (Some(s), "saved_palette".to_string()),
                        None => match self.load_builtin_scheme(is_dark, &selected_builtin_theme_id)
                        {
                            Some(s) => (Some(s), "builtin".to_string()),
                            None => (None, "none".to_string()),
                        },
                    }
                } else {
                    // palette_id 格式无效，fallback 到 builtin
                    match self.load_builtin_scheme(is_dark, &selected_builtin_theme_id) {
                        Some(s) => (Some(s), "builtin".to_string()),
                        None => (None, "none".to_string()),
                    }
                }
            } else {
                // palette_id 为空，fallback 到 builtin
                match self.load_builtin_scheme(is_dark, &selected_builtin_theme_id) {
                    Some(s) => (Some(s), "builtin".to_string()),
                    None => (None, "none".to_string()),
                }
            }
        } else {
            // color_source == "built_in"
            match self.load_builtin_scheme(is_dark, &selected_builtin_theme_id) {
                Some(s) => (Some(s), "builtin".to_string()),
                None => (None, "none".to_string()),
            }
        };

        // Issue #709 评论 5729368242: 只有 scheme.is_some() 时 resolved_scheme_kind
        // 才是 dark_scheme/light_scheme；scheme == None 时写 none，
        // 和 resolved_source=none 对齐。
        let resolved_scheme_kind = if scheme.is_some() {
            if is_dark {
                "dark_scheme"
            } else {
                "light_scheme"
            }
        } else {
            "none"
        };

        // Issue #709 评论 issue-body-709: 直接把 Option<ThemeColorSchemeDto> 存进
        // ResolvedThemeState.scheme，不再序列化成 JSON 字符串。theme_state_json()
        // 和 resolved_scheme_json() 从这一份 DTO 序列化，消除
        // scheme -> JSON string -> serde_json::Value -> 再包第二层 JSON 的低效路径。
        ResolvedThemeState {
            appearance_mode,
            system_is_dark,
            is_dark,
            color_source,
            selected_palette_id,
            selected_builtin_theme_id,
            scheme,
            resolved_source,
            resolved_scheme_kind: resolved_scheme_kind.to_string(),
        }
    }

    /// Issue #709 评论 5728916561: 从 builtin themes 加载 scheme 的辅助方法。
    ///
    /// 按 `is_dark` 选择 `dark_scheme`/`light_scheme`。`theme_id` 为空时
    /// 取第一个 builtin theme。返回 `None` 表示没有可用 builtin theme。
    fn load_builtin_scheme(
        &self,
        is_dark: bool,
        theme_id: &str,
    ) -> Option<writer_core::api::types::ThemeColorSchemeDto> {
        self.with_app(|app| {
            app.core_api().and_then(|core| {
                let themes = core.list_builtin_themes();
                let theme = if theme_id.is_empty() {
                    themes.first()
                } else {
                    themes.iter().find(|t| t.theme_id == theme_id)
                };
                theme.map(|t| {
                    if is_dark {
                        t.dark_scheme.clone()
                    } else {
                        t.light_scheme.clone()
                    }
                })
            })
        })
        .unwrap_or(None)
    }

    /// 返回缓存的 `ResolvedThemeState`；若缓存为空则重建一次并写入缓存。
    ///
    /// 用 `if let` 避免 `unwrap`/`expect`（workspace clippy 禁止
    /// `unwrap_used`/`expect_used`）。
    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    pub fn state(&self) -> ResolvedThemeState {
        if let Some(ref s) = *self.cached_state.borrow() {
            return s.clone();
        }
        let state = self.rebuild_resolved_state();
        *self.cached_state.borrow_mut() = Some(state.clone());
        state
    }

    /// Issue #702: 一次性发布完整主题状态。
    ///
    /// `is_dark` 和 `scheme` 从同一份 `cached_state` 读取并打包进一个 JSON，
    /// QML 侧只绑定这一个属性，从同一份 JSON 解析两者，彻底消除 isDark 已是
    /// true 但 scheme 还是上一套浅色值的中间状态。
    ///
    /// `scheme` 字段是 Core `ThemeColorScheme` DTO 反序列化后的对象（非字符串），
    /// 便于 QML 侧直接 `JSON.parse` 后按 key 读取颜色。当 scheme 为空（`None`）
    /// 时，`scheme` 字段为 null，QML 侧 fallback 到 isDark 派生的固定深/浅色。
    ///
    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试（`tests/`）能直接调用，
    /// 验证真实 Qt 行为（dark/light/system 切换、is_dark + scheme 统一体）。
    /// QML 绑定不受影响（`qt_property!` READ 方法签名不变）。
    ///
    /// Issue #709 评论 issue-body-709: 直接序列化 `ResolvedThemeState` 这一份
    /// 完整状态，不再走 `scheme -> JSON string -> serde_json::Value -> 再包
    /// 第二层 JSON` 的低效路径。输出 JSON 结构包含 appearance_mode,
    /// system_is_dark, is_dark, color_source, selected_builtin_theme_id,
    /// selected_palette_id, scheme（ThemeColorSchemeDto 对象或 null）。
    pub fn theme_state_json(&self) -> QString {
        let state = self.state();
        // Issue #709 评论 issue-body-709: 用 serde_json::Map 直接构造完整状态
        // JSON，不再走 scheme_json 反序列化再重新打包的低效路径。
        let mut obj = serde_json::Map::new();
        obj.insert(
            "appearance_mode".to_string(),
            serde_json::Value::String(state.appearance_mode),
        );
        obj.insert(
            "system_is_dark".to_string(),
            serde_json::Value::Bool(state.system_is_dark),
        );
        obj.insert(
            "is_dark".to_string(),
            serde_json::Value::Bool(state.is_dark),
        );
        obj.insert(
            "color_source".to_string(),
            serde_json::Value::String(state.color_source),
        );
        obj.insert(
            "selected_builtin_theme_id".to_string(),
            serde_json::Value::String(state.selected_builtin_theme_id),
        );
        obj.insert(
            "selected_palette_id".to_string(),
            serde_json::Value::String(state.selected_palette_id),
        );
        // Issue #709 评论 5728916561: 输出诊断字段，追踪 scheme 实际命中的来源和类型。
        obj.insert(
            "resolved_source".to_string(),
            serde_json::Value::String(state.resolved_source),
        );
        obj.insert(
            "resolved_scheme_kind".to_string(),
            serde_json::Value::String(state.resolved_scheme_kind),
        );
        // scheme 为 None 时序列化为 null，QML 侧 fallback 到 isDark 派生的固定色。
        let scheme_value = match state.scheme {
            Some(ref s) => serde_json::to_value(s).unwrap_or(serde_json::Value::Null),
            None => serde_json::Value::Null,
        };
        obj.insert("scheme".to_string(), scheme_value);
        let json = serde_json::to_string(&serde_json::Value::Object(obj))
            .unwrap_or_else(|_| {
                "{\"appearance_mode\":\"system\",\"system_is_dark\":false,\"is_dark\":false,\"color_source\":\"built_in\",\"selected_builtin_theme_id\":\"\",\"selected_palette_id\":\"\",\"scheme\":null}"
                    .to_string()
            });
        QString::from(json)
    }

    /// Issue #677 评论 5653315696: 输出的主题 JSON 字段名与 Core DTO 一致，统一使用 snake_case。
    ///
    /// `serde_json::to_string(&s)` 序列化的是 Core 的 `ThemeColorScheme` DTO，其字段名
    /// （`on_surface`、`on_surface_variant`、`surface_container_low` 等）由 serde 派生为
    /// snake_case。本方法不做任何 camelCase 转换，QML 侧（DesignTokens.qml）必须按
    /// snake_case key 读取。这是 Linux_Qt 与 Core 之间唯一的主题 JSON 字段名协议。
    ///
    /// Issue #701 评论 5702214893: scheme 在 `rebuild_resolved_state()` 里一次性
    /// 解析并缓存到 `cached_state.scheme`，此 getter 只读缓存，不再每次
    /// 都 borrow `AppBackend` 加载 palette/builtin。
    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    /// Issue #709 评论 issue-body-709: 从 `state().scheme` 序列化，None 时返回 `"{}"`。
    pub fn resolved_scheme_json(&self) -> QString {
        let state = self.state();
        match state.scheme {
            Some(ref s) => {
                let json = serde_json::to_string(s).unwrap_or_else(|_| "{}".to_string());
                QString::from(json)
            }
            None => QString::from("{}"),
        }
    }

    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    pub fn is_dark(&self) -> bool {
        self.state().is_dark
    }

    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    pub fn color_source(&self) -> QString {
        QString::from(self.state().color_source)
    }

    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    pub fn selected_builtin_theme_id(&self) -> QString {
        QString::from(self.state().selected_builtin_theme_id)
    }

    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    pub fn selected_palette_id(&self) -> QString {
        QString::from(self.state().selected_palette_id)
    }

    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    pub fn appearance_mode(&self) -> QString {
        QString::from(self.state().appearance_mode)
    }

    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    pub fn system_is_dark(&self) -> bool {
        self.state().system_is_dark
    }

    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    pub fn compute_is_dark(mode: &str, sys_dark: bool) -> bool {
        match mode {
            "dark" => true,
            "light" => false,
            _ => sys_dark,
        }
    }

    // ── Issue #712: QColor getter 方法 ──
    //
    // 每个 getter 从 `self.state().scheme` 读取对应字段（String 类型，hex 格式
    // 如 "#RRGGBB"），如果 scheme 为 None 或字段为空，返回 fallback QColor
    // （根据 is_dark 选择深色/浅色 fallback）。fallback 值与 DesignTokens.qml
    // 中当前的 isDark fallback 值一致。

    /// 解析 "#RRGGBB" 格式的 hex 字符串为 QColor。
    /// Qt 的 QColor 构造函数原生支持 "#RRGGBB" 格式。
    fn parse_hex_color(hex: &str) -> QColor {
        QColor::from_name(hex)
    }

    /// 辅助函数：从 scheme 读取颜色或返回 is_dark fallback。
    fn scheme_color_or_fallback(
        &self,
        scheme_field: &str,
        is_dark: bool,
        dark_fallback: (f64, f64, f64),
        light_fallback: (f64, f64, f64),
    ) -> QColor {
        let state = self.state();
        if let Some(ref scheme) = state.scheme {
            let val = match scheme_field {
                "primary" => &scheme.primary,
                "on_primary" => &scheme.on_primary,
                "primary_container" => &scheme.primary_container,
                "on_primary_container" => &scheme.on_primary_container,
                "secondary" => &scheme.secondary,
                "on_secondary" => &scheme.on_secondary,
                "secondary_container" => &scheme.secondary_container,
                "on_secondary_container" => &scheme.on_secondary_container,
                "tertiary" => &scheme.tertiary,
                "on_tertiary" => &scheme.on_tertiary,
                "tertiary_container" => &scheme.tertiary_container,
                "on_tertiary_container" => &scheme.on_tertiary_container,
                "background" => &scheme.background,
                "on_background" => &scheme.on_background,
                "surface" => &scheme.surface,
                "on_surface" => &scheme.on_surface,
                "surface_variant" => &scheme.surface_variant,
                "on_surface_variant" => &scheme.on_surface_variant,
                "surface_tint" => &scheme.surface_tint,
                "surface_dim" => &scheme.surface_dim,
                "surface_bright" => &scheme.surface_bright,
                "surface_container_lowest" => &scheme.surface_container_lowest,
                "surface_container_low" => &scheme.surface_container_low,
                "surface_container" => &scheme.surface_container,
                "surface_container_high" => &scheme.surface_container_high,
                "surface_container_highest" => &scheme.surface_container_highest,
                "inverse_surface" => &scheme.inverse_surface,
                "inverse_on_surface" => &scheme.inverse_on_surface,
                "inverse_primary" => &scheme.inverse_primary,
                "error" => &scheme.error,
                "on_error" => &scheme.on_error,
                "error_container" => &scheme.error_container,
                "on_error_container" => &scheme.on_error_container,
                "outline" => &scheme.outline,
                "outline_variant" => &scheme.outline_variant,
                "scrim" => &scheme.scrim,
                _ => "",
            };
            if !val.is_empty() {
                return Self::parse_hex_color(val);
            }
        }
        let (r, g, b) = if is_dark {
            dark_fallback
        } else {
            light_fallback
        };
        QColor::from_rgb_f(r, g, b)
    }

    pub fn primary_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "primary",
            state.is_dark,
            (0.573, 0.800, 1.000),
            (0.000, 0.392, 0.592),
        )
    }

    pub fn on_primary_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "on_primary",
            state.is_dark,
            (0.000, 0.200, 0.318),
            (1.000, 1.000, 1.000),
        )
    }

    pub fn primary_container_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "primary_container",
            state.is_dark,
            (0.000, 0.294, 0.451),
            (0.800, 0.898, 1.000),
        )
    }

    pub fn on_primary_container_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "on_primary_container",
            state.is_dark,
            (0.800, 0.898, 1.000),
            (0.000, 0.118, 0.192),
        )
    }

    pub fn secondary_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "secondary",
            state.is_dark,
            (0.722, 0.784, 0.855),
            (0.318, 0.376, 0.435),
        )
    }

    pub fn on_secondary_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "on_secondary",
            state.is_dark,
            (0.137, 0.196, 0.251),
            (1.000, 1.000, 1.000),
        )
    }

    pub fn secondary_container_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "secondary_container",
            state.is_dark,
            (0.224, 0.282, 0.341),
            (0.831, 0.894, 0.965),
        )
    }

    pub fn on_secondary_container_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "on_secondary_container",
            state.is_dark,
            (0.831, 0.894, 0.965),
            (0.055, 0.114, 0.165),
        )
    }

    pub fn tertiary_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "tertiary",
            state.is_dark,
            (0.843, 0.749, 1.000),
            (0.427, 0.341, 0.549),
        )
    }

    pub fn on_tertiary_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "on_tertiary",
            state.is_dark,
            (0.243, 0.165, 0.361),
            (1.000, 1.000, 1.000),
        )
    }

    pub fn tertiary_container_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "tertiary_container",
            state.is_dark,
            (0.333, 0.251, 0.455),
            (0.945, 0.855, 1.000),
        )
    }

    pub fn on_tertiary_container_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "on_tertiary_container",
            state.is_dark,
            (0.945, 0.855, 1.000),
            (0.149, 0.078, 0.278),
        )
    }

    pub fn background_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "background",
            state.is_dark,
            (0.102, 0.110, 0.118),
            (0.988, 0.988, 1.000),
        )
    }

    pub fn on_background_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "on_background",
            state.is_dark,
            (0.886, 0.890, 0.906),
            (0.094, 0.110, 0.125),
        )
    }

    pub fn surface_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "surface",
            state.is_dark,
            (0.102, 0.110, 0.118),
            (0.988, 0.988, 1.000),
        )
    }

    pub fn on_surface_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "on_surface",
            state.is_dark,
            (0.886, 0.890, 0.906),
            (0.094, 0.110, 0.125),
        )
    }

    pub fn surface_variant_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "surface_variant",
            state.is_dark,
            (0.259, 0.278, 0.306),
            (0.875, 0.890, 0.922),
        )
    }

    pub fn on_surface_variant_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "on_surface_variant",
            state.is_dark,
            (0.757, 0.776, 0.812),
            (0.259, 0.278, 0.306),
        )
    }

    pub fn surface_tint_qcolor(&self) -> QColor {
        let state = self.state();
        // surface_tint fallback 与 primary 相同（QML 中 `?? primary`）
        self.scheme_color_or_fallback(
            "surface_tint",
            state.is_dark,
            (0.573, 0.800, 1.000),
            (0.000, 0.392, 0.592),
        )
    }

    pub fn surface_dim_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "surface_dim",
            state.is_dark,
            (0.071, 0.078, 0.094),
            (0.843, 0.851, 0.875),
        )
    }

    pub fn surface_bright_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "surface_bright",
            state.is_dark,
            (0.220, 0.224, 0.247),
            (0.988, 0.988, 1.000),
        )
    }

    pub fn surface_container_lowest_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "surface_container_lowest",
            state.is_dark,
            (0.059, 0.067, 0.075),
            (1.000, 1.000, 1.000),
        )
    }

    pub fn surface_container_low_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "surface_container_low",
            state.is_dark,
            (0.122, 0.133, 0.145),
            (0.965, 0.973, 0.984),
        )
    }

    pub fn surface_container_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "surface_container",
            state.is_dark,
            (0.137, 0.153, 0.165),
            (0.941, 0.953, 0.969),
        )
    }

    pub fn surface_container_high_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "surface_container_high",
            state.is_dark,
            (0.176, 0.192, 0.208),
            (0.918, 0.937, 0.961),
        )
    }

    pub fn surface_container_highest_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "surface_container_highest",
            state.is_dark,
            (0.220, 0.235, 0.251),
            (0.894, 0.914, 0.937),
        )
    }

    pub fn inverse_surface_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "inverse_surface",
            state.is_dark,
            (0.886, 0.886, 0.898),
            (0.184, 0.188, 0.200),
        )
    }

    pub fn inverse_on_surface_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "inverse_on_surface",
            state.is_dark,
            (0.184, 0.188, 0.200),
            (0.945, 0.941, 0.957),
        )
    }

    pub fn inverse_primary_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "inverse_primary",
            state.is_dark,
            (0.000, 0.392, 0.592),
            (0.573, 0.800, 1.000),
        )
    }

    pub fn error_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "error",
            state.is_dark,
            (1.000, 0.706, 0.671),
            (0.729, 0.102, 0.102),
        )
    }

    pub fn on_error_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "on_error",
            state.is_dark,
            (0.412, 0.000, 0.020),
            (1.000, 1.000, 1.000),
        )
    }

    pub fn error_container_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "error_container",
            state.is_dark,
            (0.576, 0.000, 0.039),
            (1.000, 0.855, 0.839),
        )
    }

    pub fn on_error_container_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "on_error_container",
            state.is_dark,
            (1.000, 0.855, 0.839),
            (0.255, 0.000, 0.008),
        )
    }

    pub fn outline_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "outline",
            state.is_dark,
            (0.549, 0.569, 0.596),
            (0.447, 0.471, 0.494),
        )
    }

    pub fn outline_variant_qcolor(&self) -> QColor {
        let state = self.state();
        self.scheme_color_or_fallback(
            "outline_variant",
            state.is_dark,
            (0.259, 0.278, 0.306),
            (0.757, 0.776, 0.812),
        )
    }

    pub fn scrim_qcolor(&self) -> QColor {
        let state = self.state();
        // scrim fallback 深色和浅色都是黑色
        self.scheme_color_or_fallback(
            "scrim",
            state.is_dark,
            (0.000, 0.000, 0.000),
            (0.000, 0.000, 0.000),
        )
    }

    /// Issue #710 评论 5731145076: 记录完整 resolved theme 诊断事件。
    ///
    /// 每次 appearance/source/builtin/palette/system dark 真正改变 resolved state 后，
    /// 都记录完整 resolved theme，不要只有 reload() 才记。日志直接写出
    /// appearance_mode、system_is_dark、is_dark、resolved_source、resolved_scheme_kind、
    /// theme/palette id、surface、on_surface、on_surface_variant，便于定位深色模式
    /// 文字仍为黑色等问题。scheme 为 None 时颜色字段写 `<none>`。
    fn log_resolved_theme(
        &self,
        event_name: &str,
        origin: writer_diagnostics::DiagnosticOrigin,
        state: &ResolvedThemeState,
    ) {
        let is_dark_str = if state.is_dark { "true" } else { "false" };
        let system_is_dark_str = if state.system_is_dark {
            "true"
        } else {
            "false"
        };
        // scheme 为 None 时用 "<none>" 表示颜色字段缺失，便于诊断日志区分
        // "scheme 未加载" 与 "scheme 加载成功但颜色为空"。
        let (surface, on_surface, on_surface_variant) = match state.scheme {
            Some(ref s) => (
                s.surface.as_str(),
                s.on_surface.as_str(),
                s.on_surface_variant.as_str(),
            ),
            None => ("<none>", "<none>", "<none>"),
        };
        crate::backend::app_backend::record_struct_event(
            origin,
            event_name,
            "theme",
            &[
                ("appearanceMode", &state.appearance_mode),
                ("systemIsDark", system_is_dark_str),
                ("isDark", is_dark_str),
                ("resolvedSource", &state.resolved_source),
                ("resolvedSchemeKind", &state.resolved_scheme_kind),
                ("builtinThemeId", &state.selected_builtin_theme_id),
                ("paletteId", &state.selected_palette_id),
                ("surface", surface),
                ("onSurface", on_surface),
                ("onSurfaceVariant", on_surface_variant),
            ],
        );
    }

    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    pub fn reload(&mut self) {
        // 主题解析（应用内部逻辑）→ origin=App。
        let state = self.rebuild_resolved_state();
        *self.cached_state.borrow_mut() = Some(state.clone());
        // Issue #710 评论 5731145076: reload() 记录完整 resolved theme，
        // 不只记录 appearanceMode 和 isDark。
        self.log_resolved_theme(
            "theme.resolve",
            writer_diagnostics::DiagnosticOrigin::App,
            &state,
        );
        self.scheme_changed();
    }

    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    pub fn set_color_source(&mut self, val: QString) {
        let source = val.to_string();
        if source == "saved_palette" {
            let pid = self.snap().selected_palette_id.clone();
            if pid.is_empty() {
                return;
            }
        }
        if self
            .with_app_mut(|app| app.set_setting_color_source(val))
            .is_ok()
        {
            let state = self.rebuild_resolved_state();
            *self.cached_state.borrow_mut() = Some(state.clone());
            // Issue #710 评论 5731145076: color source 改变后记录完整 resolved theme。
            self.log_resolved_theme(
                "theme.color_source_resolved",
                writer_diagnostics::DiagnosticOrigin::User,
                &state,
            );
            self.scheme_changed();
        }
    }

    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    pub fn set_appearance_mode(&mut self, val: QString) {
        // 用户选择外观模式（点击 dark/light/system）→ origin=User。
        let mode_str = val.to_string();
        crate::backend::app_backend::record_struct_event(
            writer_diagnostics::DiagnosticOrigin::User,
            "theme.appearance_select",
            "theme",
            &[("requested", &mode_str)],
        );
        if self
            .with_app_mut(|app| app.set_setting_appearance_mode(val))
            .is_ok()
        {
            let state = self.rebuild_resolved_state();
            *self.cached_state.borrow_mut() = Some(state.clone());
            // Issue #710 评论 5731145076: appearance mode 改变后记录完整 resolved theme。
            self.log_resolved_theme(
                "theme.appearance_resolved",
                writer_diagnostics::DiagnosticOrigin::User,
                &state,
            );
            self.scheme_changed();
        }
    }

    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    pub fn set_selected_builtin_theme_id(&mut self, val: QString) {
        if self
            .with_app_mut(|app| app.set_setting_selected_builtin_theme_id(val))
            .is_ok()
        {
            if self
                .with_app_mut(|app| app.set_setting_color_source("built_in".into()))
                .is_err()
            {
                crate::backend::app_backend::debug_error_static(
                    "theme_controller",
                    "BORROW_CONFLICT",
                    "set_setting_color_source skipped due to borrow conflict",
                );
            }
            let state = self.rebuild_resolved_state();
            *self.cached_state.borrow_mut() = Some(state.clone());
            // Issue #710 评论 5731145076: builtin theme 改变后记录完整 resolved theme。
            self.log_resolved_theme(
                "theme.builtin_theme_resolved",
                writer_diagnostics::DiagnosticOrigin::User,
                &state,
            );
            self.scheme_changed();
        }
    }

    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    pub fn set_selected_palette_id(&mut self, val: QString) {
        let palette_id = val.to_string();
        if !palette_id.is_empty() {
            if self
                .with_app_mut(|app| app.set_setting_selected_palette_id(val))
                .is_err()
            {
                crate::backend::app_backend::debug_error_static(
                    "theme_controller",
                    "BORROW_CONFLICT",
                    "set_setting_selected_palette_id skipped due to borrow conflict",
                );
            }
            if self
                .with_app_mut(|app| app.set_setting_color_source("saved_palette".into()))
                .is_err()
            {
                crate::backend::app_backend::debug_error_static(
                    "theme_controller",
                    "BORROW_CONFLICT",
                    "set_setting_color_source skipped due to borrow conflict",
                );
            }
        }
        let state = self.rebuild_resolved_state();
        *self.cached_state.borrow_mut() = Some(state.clone());
        // Issue #710 评论 5731145076: palette 改变后记录完整 resolved theme。
        self.log_resolved_theme(
            "theme.palette_resolved",
            writer_diagnostics::DiagnosticOrigin::User,
            &state,
        );
        self.scheme_changed();
    }

    /// Issue #707 评论 5723616999: 改 `pub` 让集成测试能直接调用。
    pub fn set_system_is_dark(&mut self, val: bool) {
        // 系统主题变化回调 → origin=System。
        let is_dark_str = if val { "true" } else { "false" };
        crate::backend::app_backend::record_struct_event(
            writer_diagnostics::DiagnosticOrigin::System,
            "theme.system_color_scheme",
            "theme",
            &[("isDark", is_dark_str)],
        );
        // Issue #696 评论 5696993601: 只有 appearance_mode == "system" 时，
        // 系统深浅变化才改变有效 scheme。用户明确选 light/dark 时，系统色彩
        // 变化不能重新解释用户偏好，也不发 scheme_changed。current_system_is_dark
        // 仍然更新，以便用户切回 system 模式时用到最新值。
        let mode = self.snap().appearance_mode.clone();
        let should_emit = mode == "system";
        if self
            .with_app_mut(|app| {
                app.current_system_is_dark = val;
            })
            .is_ok()
        {
            // 始终重建缓存（current_system_is_dark 已写入 AppBackend，下次
            // getter 读取时缓存需反映最新值，即便 mode != "system" 不发信号）。
            let state = self.rebuild_resolved_state();
            *self.cached_state.borrow_mut() = Some(state.clone());
            // Issue #710 评论 5731145076: system_is_dark 改变后记录完整 resolved theme。
            self.log_resolved_theme(
                "theme.system_is_dark_resolved",
                writer_diagnostics::DiagnosticOrigin::System,
                &state,
            );
            if should_emit {
                self.scheme_changed();
            }
        }
    }
}

/// 从 `DomainSnapshot` 一次性解析出的运行时主题状态。
///
/// `LinuxThemeController` 的所有 QML 只读属性和 `resolved_scheme_json` 都
/// 基于这同一份状态，避免一个属性读 `DomainSnapshot`、另一个属性临时再
/// borrow `AppBackend` 造成的双状态机。
///
/// Issue #701 评论 5702214893: 整个 `ResolvedThemeState` 在
/// `rebuild_resolved_state()` 里一次性构造并缓存到 `cached_state`，getter
/// 只读缓存。
///
/// Issue #709 评论 issue-body-709: `scheme_json: String` 改为直接保存最终选中
/// 的 `ThemeColorSchemeDto`。`theme_state_json()` 直接序列化这一份完整状态，
/// 不再走 `scheme -> JSON string -> serde_json::Value -> 再包第二层 JSON` 的
/// 低效路径，且消除 is_dark 已是 true 但 scheme 还是浅色值的中间状态。
#[derive(Clone)]
pub struct ResolvedThemeState {
    pub appearance_mode: String,
    pub system_is_dark: bool,
    pub is_dark: bool,
    pub color_source: String,
    pub selected_palette_id: String,
    pub selected_builtin_theme_id: String,
    /// Issue #709 评论 issue-body-709: 最终选中的 ThemeColorSchemeDto（按
    /// is_dark 一次性选择 dark_scheme/light_scheme）。None 表示没有可用
    /// scheme（builtin theme 列表为空等），QML 侧 fallback 到 isDark 派生
    /// 的固定色。
    pub scheme: Option<writer_core::api::types::ThemeColorSchemeDto>,
    /// Issue #709 评论 5728916561: 实际命中的来源 — "saved_palette"、"builtin" 或 "none"。
    /// 区分 color_source（用户设置）和 resolved_source（实际命中）：
    /// color_source 可能是 "saved_palette" 但 palette_id 无效，最终 fallback 到 builtin。
    pub resolved_source: String,
    /// Issue #709 评论 5728916561: 实际选择的 scheme 类型 — "dark_scheme"、"light_scheme" 或 "none"。
    /// 确认 is_dark=true 时确实选了 dark_scheme，is_dark=false 时确实选了 light_scheme。
    pub resolved_scheme_kind: String,
}
