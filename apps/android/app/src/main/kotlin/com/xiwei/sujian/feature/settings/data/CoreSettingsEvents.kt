package com.xiwei.sujian.feature.settings.data
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow

/**
 * 设置领域事件总线（#618 三）— 按消费用途拆成三条独立事件，不再共用一个模糊全局事件。
 *
 * 旧实现把一次普通本地保存同时发成 `settingsChanged + editorSettingsChanged`，
 * 导致 SettingsViewModel 收到自己刚保存的事件又从 Core 重读整份设置、
 * ThemeController 也执行主题目录重载 —— 普通开关保存仍重跑主题解析。
 *
 * 现在：
 * - [externalSettingsChanged]：同步把设置拉回来时由外部触发，SettingsViewModel 才
 *   reloadFromExternalSync；
 * - [editorSettingsChanged]：编辑器相关设置变化（本地保存成功或外部同步），
 *   WritingPane 用它 reloadSettings。
 *
 * Issue #1005：设备配色不再跨端同步，删除 themeCatalogChanged 主题目录同步事件。
 */
object CoreSettingsEvents {
    private val _externalSettingsChanged = MutableSharedFlow<Unit>(extraBufferCapacity = 1)
    val externalSettingsChanged: SharedFlow<Unit> = _externalSettingsChanged.asSharedFlow()

    private val _editorSettingsChanged = MutableSharedFlow<Unit>(extraBufferCapacity = 1)
    val editorSettingsChanged: SharedFlow<Unit> = _editorSettingsChanged.asSharedFlow()

    /** 本机设置保存成功：只通知编辑器需要更新的部分，不冒充外部设置变化。 */
    fun notifyLocalEditorSettingsChanged() {
        _editorSettingsChanged.tryEmit(Unit)
    }

    /** 外部同步把设置拉回来：设置页重新从 Core 加载 + 编辑器刷新。 */
    fun notifyExternalSettingsChanged() {
        _externalSettingsChanged.tryEmit(Unit)
        _editorSettingsChanged.tryEmit(Unit)
    }
}
