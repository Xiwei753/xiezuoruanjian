package com.xiwei.sujian.ui

import android.content.Intent

import android.view.Menu
import android.view.MenuItem
import android.widget.FrameLayout
import android.widget.LinearLayout
import androidx.coordinatorlayout.widget.CoordinatorLayout
import com.google.android.material.appbar.MaterialToolbar
import com.google.android.material.bottomnavigation.BottomNavigationView
import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.ImageView
import android.widget.TextView
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.activity.OnBackPressedCallback
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.google.android.material.floatingactionbutton.FloatingActionButton
import com.xiwei.sujian.R
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.data.WorkspaceUseCase
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.model.Project
import com.xiwei.sujian.model.RecentEdit
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.model.LayoutPlan
import com.xiwei.sujian.model.WindowMetrics
import com.xiwei.sujian.model.Orientation
import com.xiwei.sujian.model.PointerKind
import com.xiwei.sujian.model.FoldPosture
import com.xiwei.sujian.model.ShellMode
import com.xiwei.sujian.model.ScreenRole
import com.xiwei.sujian.model.ScreenPolicy
import com.xiwei.sujian.model.ActionSlot
import com.xiwei.sujian.model.ActionRole
import com.xiwei.sujian.model.ActionPlacement
import androidx.appcompat.app.AppCompatDelegate
import android.os.Build
import android.util.Log
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * MainActivity — 应用主界面
 *
 * 包含作品列表、最近编辑、星图和统计四个标签页，是应用的导航中心。
 *
 * ## 架构定位
 * - 应用入口 Activity，管理底部导航和标签页切换
 * - 通过 Repository/领域 Bridge 与 Rust Core 交互
 *
 * ## 职责边界
 * - **做**：作品列表展示、新建作品、标签页切换、主题管理
 * - **不做**：具体业务逻辑（由各 Controller 负责）
 *
 * ## 依赖关系
 * - 领域 Bridge：Rust Core JNI 桥接（legacy adapter 仅在 data 层内部）
 * - StarMapController：星图标签页控制器
 * - StatsController：统计标签页控制器
 *
 * ## 使用场景
 * - 应用启动后的主界面
 * - 作品的创建、浏览和管理
 */
class MainActivity : AppCompatActivity() {
    private lateinit var projectRecyclerView: RecyclerView
    private lateinit var recentEditsRecyclerView: RecyclerView
    private lateinit var fabNewProject: FloatingActionButton
    private lateinit var fabNewStarMapNode: FloatingActionButton
    private lateinit var emptyStateLayout: View
    private lateinit var recentEditsLayout: View
    private lateinit var btnSettings: ImageView
    private lateinit var tabWorks: FrameLayout
    private lateinit var tabStarMap: FrameLayout
    private lateinit var tabStats: FrameLayout
    private lateinit var mainContainer: CoordinatorLayout
    private lateinit var bottomNav: BottomNavigationView
    private lateinit var canvasView: StarMapCanvasView
    private lateinit var toolbar: MaterialToolbar
    private var createProjectMenuItem: MenuItem? = null

    // ── TwoPane 模式的 View 引用 ──
    private lateinit var twoPaneContainer: LinearLayout
    private lateinit var leftPane: FrameLayout
    private lateinit var rightPane: FrameLayout
    private lateinit var paneDivider: View
    private lateinit var detailPlaceholder: View
    private lateinit var detailContentContainer: FrameLayout
    private lateinit var projectRecyclerViewLeft: RecyclerView
    private lateinit var recentEditsRecyclerViewLeft: RecyclerView
    private lateinit var emptyStateLayoutLeft: View
    private lateinit var recentEditsLayoutLeft: View
    private lateinit var tabWorksLeft: FrameLayout
    private lateinit var tabStarMapLeft: FrameLayout
    private lateinit var tabStatsLeft: FrameLayout
    private lateinit var canvasViewLeft: StarMapCanvasView
    private lateinit var leftPaneAdapter: ProjectAdapter
    private lateinit var leftPaneRecentAdapter: RecentEditAdapter

    var starmapId: String = ""
    
    private lateinit var starMapController: StarMapController
    private lateinit var statsController: StatsController

    private lateinit var workspaceRepository: WorkspaceRepository
    private lateinit var workspaceUseCase: WorkspaceUseCase
    private lateinit var settingsRepository: SettingsRepository
    private var projects = listOf<Project>()
    private var recentEdits = listOf<RecentEdit>()
    private lateinit var adapter: ProjectAdapter
    private lateinit var recentAdapter: RecentEditAdapter
    private var currentLayoutPlan: LayoutPlan? = null
    private var isTwoPaneMode: Boolean = false
    private var currentWorkspacePolicy: ScreenPolicy? = null

    // ── TwoPane 模式下当前嵌入的 EditorFragment 引用 ──
    private var currentEditorFragment: EditorFragment? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        ErrorUtil.safeRun(this) {
            settingsRepository = SettingsRepository(this)
            val settings = ErrorUtil.safeRun(this, LocalSettings()) {
                settingsRepository.getLocalSettings()
            }
            when (settings.themeMode) {
                "light" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_NO)
                "dark" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_YES)
                else -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_FOLLOW_SYSTEM)
            }
        }

        setContentView(R.layout.activity_main)


        window.decorView.post {
            UiFontUtil.applySansSerifFallback(window.decorView.rootView)
        }

        projectRecyclerView = findViewById(R.id.projectRecyclerView)
        recentEditsRecyclerView = findViewById(R.id.recentEditsRecyclerView)
        fabNewProject = findViewById(R.id.fabNewProject)
        fabNewStarMapNode = findViewById(R.id.fabNewStarMapNode)
        emptyStateLayout = findViewById(R.id.emptyStateLayout)
        recentEditsLayout = findViewById(R.id.recentEditsLayout)
        btnSettings = findViewById(R.id.btnSettings)
        tabWorks = findViewById(R.id.tabWorks)
        tabStarMap = findViewById(R.id.tabStarMap)
        tabStats = findViewById(R.id.tabStats)
        mainContainer = findViewById(R.id.mainContainer)
        bottomNav = findViewById(R.id.bottomNav)
        canvasView = findViewById(R.id.canvasView)
        toolbar = findViewById(R.id.toolbar)
        toolbar.inflateMenu(R.menu.menu_main_toolbar)
        createProjectMenuItem = toolbar.menu.findItem(R.id.action_create_project)
        toolbar.setOnMenuItemClickListener { item ->
            when (item.itemId) {
                R.id.action_create_project -> {
                    showNewProjectDialog()
                    true
                }
                else -> false
            }
        }

        // ── TwoPane 模式的 View 引用初始化 ──
        twoPaneContainer = findViewById(R.id.twoPaneContainer)
        leftPane = findViewById(R.id.leftPane)
        rightPane = findViewById(R.id.rightPane)
        paneDivider = findViewById(R.id.paneDivider)
        detailPlaceholder = findViewById(R.id.detailPlaceholder)
        detailContentContainer = findViewById(R.id.detailContentContainer)
        projectRecyclerViewLeft = findViewById(R.id.projectRecyclerViewLeft)
        recentEditsRecyclerViewLeft = findViewById(R.id.recentEditsRecyclerViewLeft)
        emptyStateLayoutLeft = findViewById(R.id.emptyStateLayoutLeft)
        recentEditsLayoutLeft = findViewById(R.id.recentEditsLayoutLeft)
        tabWorksLeft = findViewById(R.id.tabWorksLeft)
        tabStarMapLeft = findViewById(R.id.tabStarMapLeft)
        tabStatsLeft = findViewById(R.id.tabStatsLeft)
        canvasViewLeft = findViewById(R.id.canvasViewLeft)

        starMapController = StarMapController(this, com.xiwei.sujian.data.BridgeProvider.getStarmapBridge(this), tabStarMap, canvasView)
        statsController = StatsController(this, com.xiwei.sujian.data.BridgeProvider.getStatsBridge(this), tabStats)

        // Sync initial state
        when (bottomNav.selectedItemId) {
            R.id.nav_works -> {
                tabWorks.visibility = View.VISIBLE
                tabStarMap.visibility = View.GONE
                tabStats.visibility = View.GONE
                toolbar.title = getString(R.string.title_projects)
                fabNewProject.show()
                fabNewStarMapNode.hide()
            }
            R.id.nav_starmap -> {
                tabWorks.visibility = View.GONE
                tabStarMap.visibility = View.VISIBLE
                tabStats.visibility = View.GONE
                toolbar.title = getString(R.string.title_starmap)
                starMapController.initialize(starmapId)
                fabNewProject.hide()
                fabNewStarMapNode.show()
            }
            R.id.nav_stats -> {
                tabWorks.visibility = View.GONE
                tabStarMap.visibility = View.GONE
                tabStats.visibility = View.VISIBLE
                toolbar.title = getString(R.string.title_stats)
                statsController.initialize()
                fabNewProject.hide()
                fabNewStarMapNode.hide()
            }
        }

        bottomNav.setOnItemSelectedListener { item ->
            when (item.itemId) {
                R.id.nav_works -> {
                    tabWorks.visibility = View.VISIBLE
                    tabStarMap.visibility = View.GONE
                    tabStats.visibility = View.GONE
                    toolbar.title = getString(R.string.title_projects)
                    fabNewProject.show()
                    fabNewStarMapNode.hide()
                    true
                }
                R.id.nav_starmap -> {
                    tabWorks.visibility = View.GONE
                    tabStarMap.visibility = View.VISIBLE
                    tabStats.visibility = View.GONE
                    toolbar.title = getString(R.string.title_starmap)
                    starMapController.initialize(starmapId)
                    fabNewProject.hide()
                    fabNewStarMapNode.show()
                    true
                }
                R.id.nav_stats -> {
                    tabWorks.visibility = View.GONE
                    tabStarMap.visibility = View.GONE
                    tabStats.visibility = View.VISIBLE
                    toolbar.title = getString(R.string.title_stats)
                    statsController.initialize()
                    fabNewProject.hide()
                    fabNewStarMapNode.hide()
                    true
                }
                else -> false
            }
        }

        ErrorUtil.safeRun(this) {
            workspaceRepository = WorkspaceRepository(this)
            workspaceUseCase = WorkspaceUseCase(workspaceRepository)
        }

        adapter = ProjectAdapter()
        projectRecyclerView.layoutManager = LinearLayoutManager(this)
        projectRecyclerView.adapter = adapter

        recentAdapter = RecentEditAdapter()
        recentEditsRecyclerView.layoutManager = LinearLayoutManager(this)
        recentEditsRecyclerView.adapter = recentAdapter

        // ── TwoPane 左侧面板的 adapter ──
        leftPaneAdapter = ProjectAdapter()
        projectRecyclerViewLeft.layoutManager = LinearLayoutManager(this)
        projectRecyclerViewLeft.adapter = leftPaneAdapter

        leftPaneRecentAdapter = RecentEditAdapter()
        recentEditsRecyclerViewLeft.layoutManager = LinearLayoutManager(this)
        recentEditsRecyclerViewLeft.adapter = leftPaneRecentAdapter

        // ── LayoutPlan 驱动：所有 View 引用已初始化，延迟首次应用确保视图已布局 ──
        window.decorView.post {
            applyLayoutPlan()
        }

        loadProjects()
        loadRecentEdits()

        fabNewProject.setOnClickListener {
            showNewProjectDialog()
        }

        fabNewStarMapNode.setOnClickListener {
            starMapController.showNewNodeDialog()
        }



        btnSettings.setOnClickListener {
            startActivity(Intent(this, SettingsActivity::class.java))
        }

        // ── TwoPane 模式下的 Back 行为 ──
        // 当 EditorFragment 在右侧面板时，Back 键返回章节列表而非退出 MainActivity
        onBackPressedDispatcher.addCallback(this, object : androidx.activity.OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                if (isTwoPaneMode && currentEditorFragment != null) {
                    // TwoPane 模式下有 EditorFragment：返回章节列表
                    returnToChapterList()
                } else {
                    // 默认行为：退出
                    isEnabled = false
                    onBackPressedDispatcher.onBackPressed()
                }
            }
        })
    }


    fun onStarmapIdInitialized(id: String) {
        this.starmapId = id
    }

    /**
     * 通过 Core resolve_layout 获取 LayoutPlan 并应用到 UI。
     * 不允许 Android 端自己判断 isTablet 或自己发明断点。
     *
     * 三种 ShellMode 的布局策略：
     * - SinglePane：底部导航 + 单页跳转（手机竖屏）
     * - SupportingPane：底部导航 + 内容居中限宽（手机横屏/小平板）
     * - TwoPane：隐藏底部导航，左右双栏布局（大屏/折叠屏展开）
     */
    private fun applyLayoutPlan() {
        try {
            val displayMetrics = resources.displayMetrics
            val widthPx = window.decorView.width.toFloat().coerceAtLeast(displayMetrics.widthPixels.toFloat())
            val heightPx = window.decorView.height.toFloat().coerceAtLeast(displayMetrics.heightPixels.toFloat())
            val density = displayMetrics.density
            val widthVp = widthPx / density
            val heightVp = heightPx / density

            // 从 WindowInsets 获取安全区域和键盘状态
            val insets = androidx.core.view.ViewCompat.getRootWindowInsets(window.decorView)
            val safeTopVp = insets?.let {
                it.getInsets(androidx.core.view.WindowInsetsCompat.Type.systemBars()).top / density
            } ?: 0f
            val safeBottomVp = insets?.let {
                it.getInsets(androidx.core.view.WindowInsetsCompat.Type.systemBars()).bottom / density
            } ?: 0f
            val keyboardVisible = insets?.let {
                it.isVisible(androidx.core.view.WindowInsetsCompat.Type.ime())
            } ?: false

            val metrics = WindowMetrics(
                widthVp = widthVp,
                heightVp = heightVp,
                safeTopVp = safeTopVp,
                safeBottomVp = safeBottomVp,
                keyboardVisible = keyboardVisible,
                foldPosture = FoldPosture.Unknown,
                orientation = if (widthVp > heightVp) Orientation.Landscape else Orientation.Portrait,
                pointer = PointerKind.Touch
            )

            val bridge = BridgeProvider.getLayoutPolicyBridge(this)
            currentLayoutPlan = bridge.resolveLayout(metrics)

            currentLayoutPlan?.let { plan ->
                when (plan.shellMode) {
                    ShellMode.TwoPane -> applyTwoPaneLayout(plan, widthPx, density)
                    ShellMode.SupportingPane -> applySupportingPaneLayout(plan, widthPx, density)
                    ShellMode.SinglePane -> applySinglePaneLayout(plan, density)
                }

                Log.d("MainActivity", "LayoutPlan applied: shellMode=${plan.shellMode}, " +
                    "widthClass=${plan.widthClass}, showBottomBar=${plan.showBottomBar}, " +
                    "contentMaxWidth=${plan.contentMaxWidthVp}vp, pagePadding=${plan.pagePaddingVp}vp")

                // ── 消费 ScreenPolicy：根据 Core 指定的 ActionSlot 放置按钮 ──
                applyWorkspaceScreenPolicy(plan.shellMode)
            }
        } catch (e: Exception) {
            Log.w("MainActivity", "Failed to apply LayoutPlan, using defaults", e)
            // 降级为 SinglePane
            applySinglePaneLayout(null, resources.displayMetrics.density)
        }
    }

    /**
     * TwoPane 模式：左右双栏布局
     * - 隐藏底部导航和 SinglePane 主容器
     * - 显示 twoPaneContainer，左侧列表 + 右侧详情
     * - 左侧面板占 40%，右侧面板占 60%
     */
    private fun applyTwoPaneLayout(plan: LayoutPlan, widthPx: Float, density: Float) {
        isTwoPaneMode = true

        // 隐藏 SinglePane 的主容器和底部导航
        mainContainer.visibility = View.GONE
        bottomNav.visibility = View.GONE

        // 显示 TwoPane 容器
        twoPaneContainer.visibility = View.VISIBLE

        // 左侧面板权重来自 Core LayoutPlan，右侧面板权重为 (5 - primaryPaneWeight)
        val primaryWeight = plan.primaryPaneWeight
        val detailWeight = 5f - primaryWeight
        leftPane.layoutParams = LinearLayout.LayoutParams(
            0, LinearLayout.LayoutParams.MATCH_PARENT, primaryWeight
        )
        rightPane.layoutParams = LinearLayout.LayoutParams(
            0, LinearLayout.LayoutParams.MATCH_PARENT, detailWeight
        )
        paneDivider.visibility = View.VISIBLE

        // 左侧面板默认显示 Works tab
        tabWorksLeft.visibility = View.VISIBLE
        tabStarMapLeft.visibility = View.GONE
        tabStatsLeft.visibility = View.GONE

        // 根据 pagePaddingVp 设置左侧面板内边距
        val paddingPx = (plan.pagePaddingVp * density).toInt()
        projectRecyclerViewLeft.setPadding(paddingPx, paddingPx, paddingPx, paddingPx)

        // 根据 detailPanelMaxWidthVp 限制右侧面板最大宽度
        if (plan.detailPanelMaxWidthVp > 0f) {
            val maxPx = (plan.detailPanelMaxWidthVp * density).toInt()
            rightPane.layoutParams = LinearLayout.LayoutParams(
                0, LinearLayout.LayoutParams.MATCH_PARENT, detailWeight
            ).apply {
                // 右侧面板限宽居中
                val availableWidth = (widthPx * detailWeight / (primaryWeight + detailWeight)).toInt()
                if (maxPx < availableWidth) {
                    val horizontalMargin = ((availableWidth - maxPx) / 2).coerceAtLeast(0)
                    marginStart = horizontalMargin
                    marginEnd = horizontalMargin
                }
            }
        }

        // 同步数据到左侧面板的 RecyclerView
        leftPaneAdapter.notifyDataSetChanged()
        leftPaneRecentAdapter.notifyDataSetChanged()

        // 更新 toolbar
        toolbar.title = getString(R.string.title_projects)

        // TwoPane 模式下 FAB 锚定到 twoPaneContainer
        val fabLayoutParams = fabNewProject.layoutParams as? CoordinatorLayout.LayoutParams
        fabLayoutParams?.anchorId = R.id.twoPaneContainer
        fabNewProject.layoutParams = fabLayoutParams
        // FAB 的显示/隐藏由 applyWorkspaceScreenPolicy() 统一管理，此处不主动 show
        fabNewStarMapNode.hide()
    }

    /**
     * SupportingPane 模式：底部导航 + 内容居中限宽
     * - 保持底部导航
     * - 内容区域居中并限制最大宽度
     */
    private fun applySupportingPaneLayout(plan: LayoutPlan, widthPx: Float, density: Float) {
        isTwoPaneMode = false

        // 隐藏 TwoPane 容器
        twoPaneContainer.visibility = View.GONE

        // 显示 SinglePane 主容器和底部导航
        mainContainer.visibility = View.VISIBLE
        bottomNav.visibility = View.VISIBLE

        // 恢复底部导航占位（高度从 bottomNav 控件测量，不硬编码 56dp）
        mainContainer.setPadding(0, 0, 0, 0)
        val marginBottomPx = bottomNav.measuredHeight.coerceAtLeast((56 * density).toInt())
        mainContainer.layoutParams = mainContainer.layoutParams?.apply {
            if (this is ViewGroup.MarginLayoutParams) {
                bottomMargin = marginBottomPx
            }
        }

        // 根据 contentMaxWidthVp 限制内容最大宽度并居中
        if (plan.contentMaxWidthVp > 0f && plan.contentMaxWidthVp < Float.MAX_VALUE) {
            val maxPx = (plan.contentMaxWidthVp * density).toInt()
            mainContainer.layoutParams = mainContainer.layoutParams?.apply {
                if (this is ViewGroup.MarginLayoutParams) {
                    val horizontalMargin = ((widthPx - maxPx) / 2).coerceAtLeast(0f).toInt()
                    marginStart = horizontalMargin
                    marginEnd = horizontalMargin
                }
            }
        }

        // 根据 pagePaddingVp 设置内边距
        val paddingPx = (plan.pagePaddingVp * density).toInt()
        projectRecyclerView.setPadding(paddingPx, paddingPx, paddingPx, paddingPx)

        // 恢复 FAB 锚定到 mainContainer
        val fabLayoutParams = fabNewProject.layoutParams as? CoordinatorLayout.LayoutParams
        fabLayoutParams?.anchorId = R.id.mainContainer
        fabNewProject.layoutParams = fabLayoutParams
    }

    /**
     * SinglePane 模式：底部导航 + 单页跳转（默认）
     * - 保持底部导航
     * - 全宽内容区域
     */
    private fun applySinglePaneLayout(plan: LayoutPlan?, density: Float) {
        isTwoPaneMode = false

        // 隐藏 TwoPane 容器
        twoPaneContainer.visibility = View.GONE

        // 显示 SinglePane 主容器和底部导航
        mainContainer.visibility = View.VISIBLE
        bottomNav.visibility = View.VISIBLE

        // 恢复底部导航占位（高度从 bottomNav 控件测量，不硬编码 56dp）
        mainContainer.setPadding(0, 0, 0, 0)
        val marginBottomPx = bottomNav.measuredHeight.coerceAtLeast((56 * density).toInt())
        mainContainer.layoutParams = mainContainer.layoutParams?.apply {
            if (this is ViewGroup.MarginLayoutParams) {
                bottomMargin = marginBottomPx
                marginStart = 0
                marginEnd = 0
            }
        }

        // 根据 pagePaddingVp 设置内边距
        if (plan != null) {
            val paddingPx = (plan.pagePaddingVp * density).toInt()
            projectRecyclerView.setPadding(paddingPx, paddingPx, paddingPx, paddingPx)
        }

        // 恢复 FAB 锚定到 mainContainer
        val fabLayoutParams = fabNewProject.layoutParams as? CoordinatorLayout.LayoutParams
        fabLayoutParams?.anchorId = R.id.mainContainer
        fabNewProject.layoutParams = fabLayoutParams
    }

    /**
     * 消费 Workspace 页面的 ScreenPolicy。
     *
     * 从 Core 获取 ActionSlot 列表，根据 ActionPlacement 语义放置按钮：
     * - create_project → Floating (SinglePane) / TopTrailing (TwoPane)
     * - delete → ContextMenu, requiresConfirmation = true
     *
     * 不允许硬编码按钮位置，所有位置语义来自 Core。
     */
    private fun applyWorkspaceScreenPolicy(shellMode: ShellMode) {
        try {
            val screenPolicyBridge = BridgeProvider.getScreenPolicyBridge(this)
            currentWorkspacePolicy = screenPolicyBridge.resolveScreenPolicy(ScreenRole.Workspace, shellMode)

            currentWorkspacePolicy?.let { policy ->
                for (slot in policy.actionSlots) {
                    Log.d("MainActivity", "Workspace ActionSlot: actionId=${slot.actionId}, " +
                        "role=${slot.role}, placement=${slot.placement}, " +
                        "visibleIn=${slot.visibleIn}, requiresConfirmation=${slot.requiresConfirmation}")
                }

                // 根据 ActionSlot 调整 FAB 位置
                val createProjectSlot = policy.actionSlots.find { it.role == ActionRole.CreateProject }
                applyCreateProjectPlacement(createProjectSlot, shellMode)
            }
        } catch (e: Exception) {
            Log.w("MainActivity", "Failed to apply Workspace ScreenPolicy", e)
        }
    }

    /**
     * 根据 Core 指定的 ActionPlacement 放置"新建项目"按钮。
     *
     * - Floating：显示为 FAB（浮动操作按钮），隐藏 toolbar 菜单项
     * - TopTrailing：在 toolbar 右侧显示菜单项，隐藏 FAB
     * - 其他 placement：降级为 FAB
     */
    private fun applyCreateProjectPlacement(slot: ActionSlot?, shellMode: ShellMode) {
        if (slot == null) {
            // 没有 slot 信息时保持默认 FAB 行为
            fabNewProject.show()
            createProjectMenuItem?.isVisible = false
            return
        }

        // 检查当前 shellMode 是否在 visibleIn 列表中
        val isVisible = slot.visibleIn.contains(shellMode)
        if (!isVisible) {
            fabNewProject.hide()
            createProjectMenuItem?.isVisible = false
            return
        }

        when (slot.placement) {
            ActionPlacement.Floating -> {
                // Floating：显示为 FAB，隐藏 toolbar 菜单项
                fabNewProject.show()
                createProjectMenuItem?.isVisible = false
            }
            ActionPlacement.TopTrailing -> {
                // TopTrailing：隐藏 FAB，在 toolbar 右侧显示菜单项
                fabNewProject.hide()
                createProjectMenuItem?.isVisible = true
            }
            else -> {
                // 其他 placement 降级为 FAB
                fabNewProject.show()
                createProjectMenuItem?.isVisible = false
                Log.d("MainActivity", "create_project placement=${slot.placement}, fallback to FAB")
            }
        }
    }

    /**
     * 检查指定 ActionRole 是否需要确认对话框。
     * 由 Core 的 ActionSlot.requiresConfirmation 决定，不允许前端硬编码。
     */
    fun isActionConfirmationRequired(role: ActionRole): Boolean {
        val slot = currentWorkspacePolicy?.actionSlots?.find { it.role == role }
        return slot?.requiresConfirmation ?: false
    }

    /**
     * TwoPane 模式下：在右侧面板显示指定项目的章节列表。
     * 使用 DetailChapterListFragment 替代动态创建视图。
     */
    private fun showChapterListInRightPane(projectId: String, projectTitle: String) {
        // 隐藏占位提示，显示内容容器
        detailPlaceholder.visibility = View.GONE
        detailContentContainer.visibility = View.VISIBLE

        // 清空右侧面板旧内容
        detailContentContainer.removeAllViews()

        // 使用 DetailChapterListFragment
        val fragment = DetailChapterListFragment.newInstance(projectId, projectTitle)
        fragment.setOnChapterClickListener { volumeId, chapterId, chapterTitle ->
            showEditorInRightPane(projectId, volumeId, chapterId, chapterTitle)
        }

        supportFragmentManager.beginTransaction()
            .replace(R.id.detailContentContainer, fragment)
            .commit()

        // 更新 toolbar 标题
        toolbar.title = projectTitle
    }

    /**
     * TwoPane 模式下：在右侧面板嵌入 EditorFragment。
     *
     * 使用 supportFragmentManager 将 EditorFragment 替换到 detailContentContainer 中。
     * 切换章节时替换 Fragment（而非启动新 Activity）。
     * 保存当前 Fragment 引用以便后续操作（如 requestSave）。
     */
    private fun showEditorInRightPane(projectId: String, volumeId: String, chapterId: String, chapterTitle: String = "") {
        // 隐藏占位提示，显示内容容器
        detailPlaceholder.visibility = View.GONE
        detailContentContainer.visibility = View.VISIBLE

        // 如果已有 EditorFragment 且是同一章节，不重复创建
        val existingFragment = currentEditorFragment
        if (existingFragment != null && existingFragment.getCurrentChapterId() == chapterId) {
            return
        }

        // 清除右侧面板中动态添加的视图（如章节列表），避免与 Fragment 冲突
        detailContentContainer.removeAllViews()

        // 移除旧的 EditorFragment（如果有）
        currentEditorFragment?.let { oldFragment ->
            supportFragmentManager.beginTransaction()
                .remove(oldFragment)
                .commitNow()
        }

        // 创建新的 EditorFragment
        val newFragment = EditorFragment.newInstance(projectId, volumeId, chapterId, chapterTitle)
        newFragment.setCallback(object : EditorFragment.EditorFragmentCallback {
            override fun onBackRequested() {
                // TwoPane 模式下：返回章节列表（不退出 MainActivity）
                returnToChapterList()
            }
        })

        // 替换右侧面板的 Fragment
        supportFragmentManager.beginTransaction()
            .replace(R.id.detailContentContainer, newFragment)
            .commit()

        currentEditorFragment = newFragment

        // 更新 toolbar 标题
        if (chapterTitle.isNotEmpty()) {
            toolbar.title = chapterTitle
        }
    }

    /**
     * TwoPane 模式下：从编辑器返回章节列表。
     * 移除 EditorFragment 或 DetailChapterListFragment，恢复占位提示。
     */
    private fun returnToChapterList() {
        currentEditorFragment?.let { fragment ->
            // 先保存当前内容
            lifecycleScope.launch {
                fragment.requestSave().await()
                // 移除 EditorFragment
                supportFragmentManager.beginTransaction()
                    .remove(fragment)
                    .commitNow()
                currentEditorFragment = null
                restoreDetailPlaceholder()
            }
        } ?: run {
            // 清除所有 Fragment（可能是 DetailChapterListFragment）
            val currentFragment = supportFragmentManager.findFragmentById(R.id.detailContentContainer)
            if (currentFragment != null) {
                supportFragmentManager.beginTransaction()
                    .remove(currentFragment)
                    .commitNow()
            }
            restoreDetailPlaceholder()
        }
    }

    /**
     * 恢复右侧面板占位提示状态
     */
    private fun restoreDetailPlaceholder() {
        detailPlaceholder.visibility = View.VISIBLE
        detailContentContainer.visibility = View.GONE
        detailContentContainer.removeAllViews()
        toolbar.title = getString(R.string.title_projects)
    }


    override fun onConfigurationChanged(newConfig: android.content.res.Configuration) {
        super.onConfigurationChanged(newConfig)
        // 配置变化（旋转、折叠）时重新应用 LayoutPlan
        applyLayoutPlan()
    }

    override fun onResume() {
        super.onResume()
        loadProjects()
        loadRecentEdits()
        syncMonetColor()
    }

    private fun syncMonetColor() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            lifecycleScope.launch {
                try {
                    val colorInt = resources.getColor(android.R.color.system_accent1_500, theme)
                    val hexColor = String.format("#%06X", 0xFFFFFF and colorInt)

                    if (::settingsRepository.isInitialized) {
                        withContext(Dispatchers.IO) {
                            val syncable = settingsRepository.getSyncableSettings()
                            if (syncable.monetColor != hexColor) {
                                settingsRepository.saveSyncableSettings(syncable.copy(monetColor = hexColor))
                            }
                        }
                    }
                } catch (e: Exception) {
                    Log.w("MainActivity", "Failed to extract Monet color", e)
                }
            }
        }
    }

    private fun loadRecentEdits() {
        lifecycleScope.launch {
            recentEdits = ErrorUtil.safeRunSuspend(this@MainActivity, emptyList()) {
                if (::workspaceUseCase.isInitialized) {
                    workspaceUseCase.getRecentEdits(3)
                } else {
                    emptyList()
                }
            }

            if (recentEdits.isEmpty()) {
                recentEditsLayout.visibility = View.GONE
                if (isTwoPaneMode && ::recentEditsLayoutLeft.isInitialized) {
                    recentEditsLayoutLeft.visibility = View.GONE
                }
            } else {
                recentEditsLayout.visibility = View.VISIBLE
                recentAdapter.notifyDataSetChanged()
                if (isTwoPaneMode && ::recentEditsLayoutLeft.isInitialized) {
                    recentEditsLayoutLeft.visibility = View.VISIBLE
                    leftPaneRecentAdapter.notifyDataSetChanged()
                }
            }
        }
    }

    private fun loadProjects() {
        lifecycleScope.launch {
            projects = ErrorUtil.safeRunSuspend(this@MainActivity, emptyList()) {
                if (::workspaceUseCase.isInitialized) {
                    workspaceUseCase.getProjects()
                } else {
                    emptyList()
                }
            }

            if (projects.isEmpty()) {
                projectRecyclerView.visibility = View.GONE
                emptyStateLayout.visibility = View.VISIBLE
                if (isTwoPaneMode && ::projectRecyclerViewLeft.isInitialized) {
                    projectRecyclerViewLeft.visibility = View.GONE
                    emptyStateLayoutLeft.visibility = View.VISIBLE
                }
            } else {
                projectRecyclerView.visibility = View.VISIBLE
                emptyStateLayout.visibility = View.GONE
                adapter.notifyDataSetChanged()
                if (isTwoPaneMode && ::projectRecyclerViewLeft.isInitialized) {
                    projectRecyclerViewLeft.visibility = View.VISIBLE
                    emptyStateLayoutLeft.visibility = View.GONE
                    leftPaneAdapter.notifyDataSetChanged()
                }
            }
        }
    }

    private fun showNewProjectDialog() {
        val editText = EditText(this)
        editText.hint = getString(R.string.hint_project_title)
        editText.setPadding(48, 48, 48, 48)

        AlertDialog.Builder(this)
            .setTitle(R.string.dialog_new_project_title)
            .setView(editText)
            .setPositiveButton(R.string.action_create) { _, _ ->
                val title = editText.text.toString().trim()
                if (title.isNotEmpty()) {
                    lifecycleScope.launch {
                        ErrorUtil.safeRunSuspend(this@MainActivity) {
                            workspaceUseCase.createProject(title)
                        }
                        loadProjects()
                    }
                }
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }


    private fun showProjectMenu(view: View, project: Project, position: Int) {
        val popup = android.widget.PopupMenu(this, view)
        popup.menu.add(0, 1, 0, getString(R.string.action_rename))
        popup.menu.add(0, 2, 0, getString(R.string.action_delete))
        if (position > 0) {
            popup.menu.add(0, 3, 0, getString(R.string.action_move_up))
        }
        if (position < projects.size - 1) {
            popup.menu.add(0, 4, 0, getString(R.string.action_move_down))
        }

        popup.setOnMenuItemClickListener { item ->
            when (item.itemId) {
                1 -> {
                    showRenameProjectDialog(project)
                    true
                }
                2 -> {
                    showDeleteProjectDialog(project)
                    true
                }
                3 -> {
                    moveProjectUp(position)
                    true
                }
                4 -> {
                    moveProjectDown(position)
                    true
                }
                else -> false
            }
        }
        popup.show()
    }

    private fun showRenameProjectDialog(project: Project) {
        val editText = android.widget.EditText(this)
        editText.setText(project.title)
        editText.setSelection(project.title.length)
        editText.hint = getString(R.string.hint_project_title)
        editText.setPadding(48, 48, 48, 48)

        AlertDialog.Builder(this)
            .setTitle(R.string.action_rename)
            .setView(editText)
            .setPositiveButton(R.string.action_ok) { _, _ ->
                val newTitle = editText.text.toString().trim()
                if (newTitle.isNotEmpty() && newTitle != project.title) {
                    lifecycleScope.launch {
                        ErrorUtil.safeRunSuspend(this@MainActivity) {
                            workspaceUseCase.renameProject(project.id, newTitle)
                        }
                        loadProjects()
                    }
                }
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }

    private fun showDeleteProjectDialog(project: Project) {
        // 由 Core ScreenPolicy 决定是否需要确认，不允许前端硬编码
        val requiresConfirmation = isActionConfirmationRequired(ActionRole.Delete)

        if (requiresConfirmation) {
            AlertDialog.Builder(this)
                .setTitle(R.string.confirm_delete_project)
                .setMessage(getString(R.string.confirm_delete_project_message, project.title))
                .setPositiveButton(R.string.action_delete) { _, _ ->
                    lifecycleScope.launch {
                        ErrorUtil.safeRunSuspend(this@MainActivity) {
                            workspaceUseCase.deleteProject(project.id)
                        }
                        loadProjects()
                    }
                }
                .setNegativeButton(R.string.action_cancel, null)
                .show()
        } else {
            // Core 指定不需要确认，直接执行
            lifecycleScope.launch {
                ErrorUtil.safeRunSuspend(this@MainActivity) {
                    workspaceUseCase.deleteProject(project.id)
                }
                loadProjects()
            }
        }
    }

    private fun moveProjectUp(position: Int) {
        if (position <= 0) return
        val orderedIds = projects.map { it.id }.toMutableList()
        val temp = orderedIds[position]
        orderedIds[position] = orderedIds[position - 1]
        orderedIds[position - 1] = temp

        lifecycleScope.launch {
            ErrorUtil.safeRunSuspend(this@MainActivity) {
                workspaceUseCase.reorderProjects(orderedIds)
            }
            loadProjects()
        }
    }

    private fun moveProjectDown(position: Int) {
        if (position >= projects.size - 1) return
        val orderedIds = projects.map { it.id }.toMutableList()
        val temp = orderedIds[position]
        orderedIds[position] = orderedIds[position + 1]
        orderedIds[position + 1] = temp

        lifecycleScope.launch {
            ErrorUtil.safeRunSuspend(this@MainActivity) {
                workspaceUseCase.reorderProjects(orderedIds)
            }
            loadProjects()
        }
    }

    private inner class ProjectAdapter : RecyclerView.Adapter<ProjectAdapter.ProjectViewHolder>() {

        inner class ProjectViewHolder(itemView: View) : RecyclerView.ViewHolder(itemView) {
            val tvProjectTitle: TextView = itemView.findViewById(R.id.tvProjectTitle)
            val tvProjectDate: TextView = itemView.findViewById(R.id.tvProjectDate)
            val btnMoreProject: android.widget.ImageButton = itemView.findViewById(R.id.btnMoreProject)

            init {
                itemView.isHapticFeedbackEnabled = false
                btnMoreProject.isHapticFeedbackEnabled = false

                btnMoreProject.setOnClickListener {
                    val pos = adapterPosition
                    if (pos != RecyclerView.NO_POSITION) {
                        val project = projects[pos]
                        showProjectMenu(btnMoreProject, project, pos)
                    }
                }

                itemView.setOnClickListener {
                    val pos = adapterPosition
                    if (pos != RecyclerView.NO_POSITION) {
                        val selectedProject = projects[pos]
                        if (isTwoPaneMode) {
                            showChapterListInRightPane(selectedProject.id, selectedProject.title)
                        } else {
                            val intent = Intent(this@MainActivity, ChapterListActivity::class.java).apply {
                                putExtra("PROJECT_ID", selectedProject.id)
                                putExtra("PROJECT_TITLE", selectedProject.title)
                            }
                            startActivity(intent)
                        }
                    }
                }

                itemView.setOnLongClickListener {
                    val pos = adapterPosition
                    if (pos != RecyclerView.NO_POSITION) {
                        val project = projects[pos]
                        showProjectMenu(itemView, project, pos)
                    }
                    true
                }
            }
        }

        override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ProjectViewHolder {
            val view = LayoutInflater.from(parent.context).inflate(R.layout.item_project, parent, false)
            return ProjectViewHolder(view)
        }

        override fun onBindViewHolder(holder: ProjectViewHolder, position: Int) {
            val project = projects[position]
            holder.tvProjectTitle.text = project.title
            // Simplified date display for MVP
            holder.tvProjectDate.text = project.updatedAt.substringBefore("T")
        }

        override fun getItemCount() = projects.size
    }

    private inner class RecentEditAdapter : RecyclerView.Adapter<RecentEditAdapter.RecentViewHolder>() {

        inner class RecentViewHolder(itemView: View) : RecyclerView.ViewHolder(itemView) {
            val tvRecentTitle: TextView = itemView.findViewById(R.id.tvRecentTitle)
            val tvRecentSubtitle: TextView = itemView.findViewById(R.id.tvRecentSubtitle)

            init {
                itemView.setOnClickListener {
                    val edit = recentEdits[adapterPosition]
                    if (isTwoPaneMode) {
                        showEditorInRightPane(edit.projectId, edit.volumeId, edit.chapterId)
                    } else {
                        val intent = Intent(this@MainActivity, EditorActivity::class.java).apply {
                            putExtra("PROJECT_ID", edit.projectId)
                            putExtra("VOLUME_ID", edit.volumeId)
                            putExtra("CHAPTER_ID", edit.chapterId)
                        }
                        startActivity(intent)
                    }
                }
            }
        }

        override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): RecentViewHolder {
            val view = LayoutInflater.from(parent.context).inflate(R.layout.item_recent_edit, parent, false)
            return RecentViewHolder(view)
        }

        override fun onBindViewHolder(holder: RecentViewHolder, position: Int) {
            val edit = recentEdits[position]
            // We just show the IDs if we don't have the titles handy,
            // but ideally we should fetch the titles. For MVP, we'll try to find the project title.
            val project = projects.find { it.id == edit.projectId }
            holder.tvRecentTitle.text = project?.title ?: "未知作品"
            holder.tvRecentSubtitle.text = "继续编写..."
        }

        override fun getItemCount() = recentEdits.size
    }
}
