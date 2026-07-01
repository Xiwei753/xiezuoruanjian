package com.xiwei.sujian.ui

import android.os.Bundle
import android.util.Log
import androidx.activity.OnBackPressedCallback
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.app.AppCompatDelegate
import com.google.android.material.appbar.MaterialToolbar
import com.xiwei.sujian.R
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.model.ScreenRole
import com.xiwei.sujian.model.ScreenPolicy
import com.xiwei.sujian.ui.system.SystemBarsController
import com.xiwei.sujian.model.ActionSlot
import com.xiwei.sujian.model.ActionRole
import com.xiwei.sujian.model.ActionPlacement
import com.xiwei.sujian.model.ShellMode
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.launch

/**
 * EditorActivity — 章节编辑器页面（SinglePane 手机模式）
 *
 * 作为 EditorFragment 的宿主 Activity，仅负责：
 * - 创建并嵌入 EditorFragment
 * - 设置独立 toolbar 和返回导航
 * - 处理 back press（保存后退出）
 *
 * ## 架构定位
 * - EditorActivity → EditorFragment → EditorViewModel → WorkspaceRepository → Rust Core
 *
 * ## 使用场景
 * - SinglePane 模式下，用户点击章节后进入独立编辑器页面
 * - TwoPane 模式下不使用此 Activity，直接在 MainActivity 右侧面板嵌入 EditorFragment
 */
class EditorActivity : AppCompatActivity(), EditorFragment.EditorFragmentCallback {

    private lateinit var toolbar: MaterialToolbar
    private var editorFragment: EditorFragment? = null
    private var currentWritingPolicy: ScreenPolicy? = null

    // ── SystemBarsController: 存为属性，供 EditorFragment 访问 ──
    val systemBarsController: SystemBarsController by lazy { SystemBarsController(this) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // 在 setContentView 前应用主题
        ErrorUtil.safeRun(this) {
            val settingsRepository = SettingsRepository(this)
            val settings = ErrorUtil.safeRun(this, LocalSettings()) {
                settingsRepository.getLocalSettings()
            }
            when (settings.themeMode) {
                "light" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_NO)
                "dark" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_YES)
                else -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_FOLLOW_SYSTEM)
            }
        }

        setContentView(R.layout.activity_editor)

        // ── SystemBarsController: edge-to-edge + insets ──
        systemBarsController.setupEdgeToEdge()

        window.decorView.post {
            UiFontUtil.applySansSerifFallback(window.decorView.rootView)
        }

        val appBarLayout = findViewById<com.google.android.material.appbar.AppBarLayout>(R.id.appBarLayout)
        systemBarsController.addAppBarTarget(appBarLayout)

        // ── 实验室全屏模式 ──
        ErrorUtil.safeRun(this) {
            val settingsRepository = SettingsRepository(this)
            val settings = settingsRepository.getLocalSettings()
            if (settings.experimentalFullscreenMode) {
                systemBarsController.applyFullscreen(true)
            }
        }

        toolbar = findViewById(R.id.toolbar)
        setSupportActionBar(toolbar)
        toolbar.setNavigationOnClickListener {
            onBackPressedDispatcher.onBackPressed()
        }

        // ── 从 Intent 获取参数 ──
        val projectId = intent.getStringExtra("PROJECT_ID")
        val volumeId = intent.getStringExtra("VOLUME_ID")
        val chapterId = intent.getStringExtra("CHAPTER_ID")
        val chapterTitle = intent.getStringExtra("CHAPTER_TITLE") ?: getString(R.string.title_editor)

        supportActionBar?.title = chapterTitle

        // ── 创建并嵌入 EditorFragment ──
        if (savedInstanceState == null) {
            editorFragment = EditorFragment.newInstance(
                projectId ?: "",
                volumeId ?: "",
                chapterId ?: "",
                chapterTitle
            )
            editorFragment?.setCallback(this)

            supportFragmentManager.beginTransaction()
                .replace(R.id.editorFragmentContainer, editorFragment!!)
                .commit()
        } else {
            // 从配置变化中恢复 Fragment 引用
            editorFragment = supportFragmentManager.findFragmentById(R.id.editorFragmentContainer) as? EditorFragment
            editorFragment?.setCallback(this)
        }

        setupBackPressed()

        // ── 消费 Writing 页面的 ScreenPolicy ──
        applyWritingScreenPolicy()
    }

    private fun setupBackPressed() {
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                val fragment = editorFragment ?: run {
                    // 没有 Fragment，直接退出
                    finish()
                    return
                }
                // 请求保存并在完成后退出
                lifecycleScope.launch {
                    val success = fragment.requestSave().await()
                    if (success) {
                        finish()
                    }
                }
            }
        })
    }

    // ── EditorFragmentCallback ──

    override fun onBackRequested() {
        finish()
    }

    // ── ScreenPolicy 消费 ──

    /**
     * 消费 Writing 页面的 ScreenPolicy。
     *
     * 从 Core 获取 ActionSlot 列表，根据 ActionPlacement 语义放置按钮：
     * - back → TopLeading（toolbar 返回按钮）
     * - save → TopTrailing（toolbar 保存按钮）
     *
     * 不允许硬编码按钮位置，所有位置语义来自 Core。
     * 第一版：读取 ScreenPolicy 并 log 结果，用简单条件判断调整按钮位置。
     */
    private fun applyWritingScreenPolicy() {
        try {
            // EditorActivity 是 SinglePane 模式
            val shellMode = ShellMode.SinglePane
            val screenPolicyBridge = BridgeProvider.getScreenPolicyBridge(this)
            currentWritingPolicy = screenPolicyBridge.resolveScreenPolicy(ScreenRole.Writing, shellMode)

            currentWritingPolicy?.let { policy ->
                for (slot in policy.actionSlots) {
                    Log.d("EditorActivity", "Writing ActionSlot: actionId=${slot.actionId}, " +
                        "role=${slot.role}, placement=${slot.placement}, " +
                        "visibleIn=${slot.visibleIn}, requiresConfirmation=${slot.requiresConfirmation}")
                }

                // ── back → TopLeading ──
                val backSlot = policy.actionSlots.find { it.role == ActionRole.Back }
                applyBackPlacement(backSlot, shellMode)

                // ── save → TopTrailing ──
                val saveSlot = policy.actionSlots.find { it.role == ActionRole.Save }
                applySavePlacement(saveSlot, shellMode)
            }
        } catch (e: Exception) {
            Log.w("EditorActivity", "Failed to apply Writing ScreenPolicy", e)
        }
    }

    /**
     * 根据 Core 指定的 ActionPlacement 放置"返回"按钮。
     *
     * - TopLeading：toolbar 左侧导航按钮（默认行为）
     * - 其他 placement：降级为 toolbar 导航
     */
    private fun applyBackPlacement(slot: ActionSlot?, shellMode: ShellMode) {
        if (slot == null) {
            // 没有 slot 信息时保持默认 toolbar 导航
            return
        }

        val isVisible = slot.visibleIn.contains(shellMode)
        if (!isVisible) {
            // Core 指定此 shellMode 下不显示 back 按钮
            toolbar.navigationIcon = null
            return
        }

        when (slot.placement) {
            ActionPlacement.TopLeading -> {
                // TopLeading：toolbar 左侧导航按钮（标准行为）
                toolbar.setNavigationOnClickListener {
                    onBackPressedDispatcher.onBackPressed()
                }
            }
            else -> {
                // 其他 placement 降级为 toolbar 导航
                Log.d("EditorActivity", "back placement=${slot.placement}, fallback to TopLeading")
                toolbar.setNavigationOnClickListener {
                    onBackPressedDispatcher.onBackPressed()
                }
            }
        }
    }

    /**
     * 根据 Core 指定的 ActionPlacement 放置"保存"按钮。
     *
     * - TopTrailing：toolbar 右侧（第一版 log 提示，后续可添加 toolbar 按钮）
     */
    private fun applySavePlacement(slot: ActionSlot?, shellMode: ShellMode) {
        if (slot == null) {
            return
        }

        val isVisible = slot.visibleIn.contains(shellMode)
        if (!isVisible) {
            Log.d("EditorActivity", "save not visible in $shellMode, skipping")
            return
        }

        when (slot.placement) {
            ActionPlacement.TopTrailing -> {
                // TopTrailing：toolbar 右侧
                // 第一版仅 log，后续可在 toolbar 右侧添加显式保存按钮
                Log.d("EditorActivity", "save placement=TopTrailing, will add toolbar save button in future iteration")
            }
            else -> {
                Log.d("EditorActivity", "save placement=${slot.placement}, no specific UI mapping yet")
            }
        }
    }

    /**
     * 检查指定 ActionRole 是否需要确认对话框。
     * 由 Core 的 ActionSlot.requiresConfirmation 决定，不允许前端硬编码。
     */
    fun isActionConfirmationRequired(role: ActionRole): Boolean {
        val slot = currentWritingPolicy?.actionSlots?.find { it.role == role }
        return slot?.requiresConfirmation ?: false
    }
}
