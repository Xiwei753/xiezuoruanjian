package com.xiwei.sujian.ui

import android.os.Bundle

import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import android.view.LayoutInflater
import android.view.Menu
import android.view.MenuInflater
import android.view.MenuItem
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.ImageButton
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.fragment.app.Fragment
import androidx.fragment.app.viewModels
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.data.CoreSettingsEvents
import com.xiwei.sujian.data.SyncChangeBus
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.editor.selfrender.SujianEditorView
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch

/**
 * EditorFragment — 可嵌入的章节编辑器 Fragment
 *
 * 从 EditorActivity 提取编辑器核心逻辑，支持两种使用模式：
 * - **TwoPane 模式**：嵌入 MainActivity 右侧面板（detailContentContainer）
 * - **SinglePane 模式**：由 EditorActivity 包裹，提供独立 toolbar
 *
 * ## 架构定位
 * - EditorFragment → EditorViewModel → WorkspaceRepository → 领域 Bridge → Rust Core
 * - EditorFragment → SujianEditorView（自研写作区唯一主路径）
 *
 * ## 职责边界
 * - **做**：文本编辑、自动保存、搜索/替换、工具栏交互
 * - **不做**：文件 I/O（由 Rust Core 负责）、排版格式化（由 SujianEditorView 负责）
 *
 * ## 宿主通信
 * - 通过 `EditorFragmentCallback` 回调宿主（如 back 请求）
 * - 通过 `initChapter()` 由宿主初始化章节
 * - 通过 `requestSave()` 由宿主触发保存
 */
class EditorFragment : Fragment() {

    companion object {
        const val ARG_PROJECT_ID = "PROJECT_ID"
        const val ARG_VOLUME_ID = "VOLUME_ID"
        const val ARG_CHAPTER_ID = "CHAPTER_ID"
        const val ARG_CHAPTER_TITLE = "CHAPTER_TITLE"

        /**
         * 工厂方法：创建带参数的 EditorFragment 实例
         */
        fun newInstance(
            projectId: String,
            volumeId: String,
            chapterId: String,
            chapterTitle: String
        ): EditorFragment {
            return EditorFragment().apply {
                arguments = Bundle().apply {
                    putString(ARG_PROJECT_ID, projectId)
                    putString(ARG_VOLUME_ID, volumeId)
                    putString(ARG_CHAPTER_ID, chapterId)
                    putString(ARG_CHAPTER_TITLE, chapterTitle)
                }
            }
        }
    }

    /**
     * 宿主回调接口
     */
    interface EditorFragmentCallback {
        /** 用户请求返回（如 back 键、toolbar 导航按钮） */
        fun onBackRequested()
    }

    // ── Views ──
    private lateinit var sujianEditorView: SujianEditorView
    private lateinit var tvWordCount: TextView
    private lateinit var tvSessionAdded: TextView
    private lateinit var tvSpeed: TextView
    private lateinit var tvSaveStatus: TextView

    // Search and Replace
    private lateinit var searchLayout: LinearLayout
    private lateinit var etSearch: EditText
    private lateinit var etReplace: EditText
    private lateinit var btnSearchNext: ImageButton
    private lateinit var btnSearchClose: ImageButton
    private lateinit var btnReplace: Button
    private lateinit var btnReplaceAll: Button

    // ── State ──
    private val viewModel: EditorViewModel by viewModels()
    private var projectId: String? = null
    private var volumeId: String? = null
    private var chapterId: String? = null
    private lateinit var workspaceRepository: WorkspaceRepository

    private var searchResults = mutableListOf<Pair<Int, Int>>()
    private var currentSearchIndex = -1

    private var callback: EditorFragmentCallback? = null

    // ── Settings cache ──
    private var lastFontSize: Float? = null
    private var lastLineSpacing: Float? = null
    private var lastTypingAnimEnabled: Boolean? = null
    private var lastTypingAnimDuration: Long? = null
    private var lastSmoothCursorEnabled: Boolean? = null
    private var lastSmoothCursorDuration: Long? = null
    private var lastAutoIndentEnabled: Boolean? = null
    private var lastAutoIndentWidth: Float? = null
    private var lastCoordinatedAnimEnabled: Boolean? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setHasOptionsMenu(true)

        // 从 arguments 读取初始参数
        projectId = arguments?.getString(ARG_PROJECT_ID)
        volumeId = arguments?.getString(ARG_VOLUME_ID)
        chapterId = arguments?.getString(ARG_CHAPTER_ID)
    }

    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?
    ): View? {
        return inflater.inflate(R.layout.fragment_editor, container, false)
    }

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        super.onViewCreated(view, savedInstanceState)

        workspaceRepository = WorkspaceRepository(requireContext())

        // ── Bind views ──
        sujianEditorView = view.findViewById(R.id.sujianEditorView)
        tvWordCount = view.findViewById(R.id.tvWordCount)
        tvSessionAdded = view.findViewById(R.id.tvSessionAdded)
        tvSpeed = view.findViewById(R.id.tvSpeed)
        tvSaveStatus = view.findViewById(R.id.tvSaveStatus)

        searchLayout = view.findViewById(R.id.searchLayout)
        etSearch = view.findViewById(R.id.etSearch)
        etReplace = view.findViewById(R.id.etReplace)
        btnSearchNext = view.findViewById(R.id.btnSearchNext)
        btnSearchClose = view.findViewById(R.id.btnSearchClose)
        btnReplace = view.findViewById(R.id.btnReplace)
        btnReplaceAll = view.findViewById(R.id.btnReplaceAll)

        // ── 设置自研写作区（唯一主路径） ──
        setupSelfRenderEditor()

        // ── Apply font fallback ──
        view.post {
            UiFontUtil.applySansSerifFallback(view)
        }

        // ── Window insets ──
        val editorStatusBar = view.findViewById<View>(R.id.editorStatusBar)
        ViewCompat.setOnApplyWindowInsetsListener(view) { v, insets ->
            val imeInsets = insets.getInsets(WindowInsetsCompat.Type.ime())
            val systemBarsInsets = insets.getInsets(WindowInsetsCompat.Type.systemBars())

            val bottomInset = maxOf(imeInsets.bottom, systemBarsInsets.bottom)
            val params = editorStatusBar.layoutParams as android.view.ViewGroup.MarginLayoutParams
            params.bottomMargin = bottomInset + (16 * resources.displayMetrics.density).toInt()
            editorStatusBar.layoutParams = params

            WindowInsetsCompat.CONSUMED
        }

        setupSearchAndReplace()
        observeViewModel()

        // ── Initialize chapter from arguments if available ──
        val chapterTitle = arguments?.getString(ARG_CHAPTER_TITLE) ?: ""
        if (projectId != null && volumeId != null && chapterId != null) {
            viewModel.initChapter(projectId!!, volumeId!!, chapterId!!, chapterTitle)
        } else {
            viewModel.initErrorState(getString(R.string.error_missing_chapter_identifiers))
        }
    }

    /**
     * 设置自研写作区 SujianEditorView
     */
    private fun setupSelfRenderEditor() {
        DiagnosticsLogger.d("SujianEditor", "Setting up self-render editor (SujianEditorView)")

        // 注入 Core 视觉事务提供者
        try {
            val animBridge = com.xiwei.sujian.data.BridgeProvider.getEditorAnimationBridge(requireContext())

            // 注入 Core 视觉事务提供者（唯一主路径）
            sujianEditorView.setVisualTransactionProvider(VisualTransactionProvider { oldText, newText, oldCursorIndex, newCursorIndex, cause, maxAnimatedChars, animationDurationMs ->
                try {
                    when (val result = animBridge.editorVisualTransaction(oldText, newText, oldCursorIndex, newCursorIndex, cause, maxAnimatedChars, animationDurationMs)) {
                        is com.xiwei.sujian.data.BridgeResult.Success -> result.data
                        else -> null
                    }
                } catch (_: Exception) {
                    null
                }
            })
            DiagnosticsLogger.d("SujianEditor", "VisualTransactionProvider injected for SujianEditorView")
        } catch (e: Exception) {
            DiagnosticsLogger.w("SujianEditor", "Failed to inject VisualTransactionProvider for SujianEditorView", e)
        }

        // 内容变更监听
        sujianEditorView.onContentChanged = { newText ->
            if (projectId != null && volumeId != null && chapterId != null) {
                viewModel.onContentChanged(newText)
            }
        }
    }

    override fun onResume() {
        super.onResume()

        if (SyncChangeBus.consumeChanged()) {
            val pid = projectId
            val vid = volumeId
            val cid = chapterId
            if (pid != null && vid != null && cid != null) {
                try {
                    workspaceRepository.getChapterContentWithMeta(pid, vid, cid)
                } catch (_: Exception) {
                    Toast.makeText(requireContext(), getString(R.string.error_chapter_deleted_remotely), Toast.LENGTH_LONG).show()
                    callback?.onBackRequested()
                    return
                }
            }
        }
        if (CoreSettingsEvents.consumeEditorChanged()) {
            viewModel.onSettingsChanged()
        }
    }

    // ── Public API ──

    /**
     * 初始化/切换章节。TwoPane 模式下切换章节时调用。
     */
    fun initChapter(projectId: String, volumeId: String, chapterId: String, chapterTitle: String) {
        this.projectId = projectId
        this.volumeId = volumeId
        this.chapterId = chapterId
        viewModel.initChapter(projectId, volumeId, chapterId, chapterTitle)
    }

    /**
     * 请求保存当前内容。返回 Deferred<Boolean>，true 表示保存成功。
     */
    fun requestSave(): Deferred<Boolean> {
        return viewModel.requestSave()
    }

    /**
     * 获取当前章节 ID
     */
    fun getCurrentChapterId(): String? = chapterId

    /**
     * 设置宿主回调
     */
    fun setCallback(callback: EditorFragmentCallback) {
        this.callback = callback
    }

    /**
     * 切换搜索栏可见性（供宿主 toolbar 菜单调用）
     */
    fun toggleSearchBar() {
        if (searchLayout.visibility == View.VISIBLE) {
            searchLayout.visibility = View.GONE
            clearHighlights()
        } else {
            searchLayout.visibility = View.VISIBLE
            etSearch.requestFocus()
            performSearch()
        }
    }

    /**
     * 显示章节备注对话框
     */
    fun showChapterNoteDialog() {
        val pid = projectId
        val vid = volumeId
        val cid = chapterId
        if (pid == null || vid == null || cid == null) return

        val currentNote = viewModel.uiState.value.chapterNote ?: ""

        val editText = EditText(requireContext())
        editText.hint = getString(R.string.hint_chapter_note)
        editText.setPadding(48, 48, 48, 48)
        editText.inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE
        editText.minLines = 3
        editText.maxLines = 10
        editText.gravity = android.view.Gravity.TOP or android.view.Gravity.START
        editText.setText(currentNote)

        AlertDialog.Builder(requireContext())
            .setTitle(R.string.dialog_chapter_note_title)
            .setView(editText)
            .setPositiveButton(R.string.action_save) { _, _ ->
                val newNote = editText.text.toString().trim()
                viewModel.updateChapterNote(newNote)
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }

    // ── Options Menu ──

    override fun onCreateOptionsMenu(menu: Menu, inflater: MenuInflater) {
        inflater.inflate(R.menu.menu_editor, menu)
        super.onCreateOptionsMenu(menu, inflater)
    }

    override fun onOptionsItemSelected(item: MenuItem): Boolean {
        return when (item.itemId) {
            R.id.action_note -> {
                showChapterNoteDialog()
                true
            }
            R.id.action_search -> {
                toggleSearchBar()
                true
            }
            R.id.action_settings -> {
                val intent = android.content.Intent(requireContext(), SettingsActivity::class.java)
                startActivity(intent)
                true
            }
            R.id.action_registry -> {
                val intent = android.content.Intent(requireContext(), ActionRegistryActivity::class.java)
                startActivity(intent)
                true
            }
            else -> super.onOptionsItemSelected(item)
        }
    }

    // ── ViewModel Observation ──

    private fun observeViewModel() {
        viewLifecycleOwner.lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                viewModel.uiState.collectLatest { state ->
                    if (state.loading) {
                        tvSaveStatus.text = ""
                        return@collectLatest
                    }

                    // 自研写作区：加载内容
                    if (!sujianEditorView.hasFocus()) {
                        if (sujianEditorView.getText() != state.content) {
                            sujianEditorView.setText(state.content)
                        }
                    }

                    when (state.saveStatus) {
                        SaveStatus.Idle -> tvSaveStatus.text = ""
                        SaveStatus.Unsaved -> tvSaveStatus.text = getString(R.string.status_unsaved)
                        SaveStatus.Saving -> tvSaveStatus.text = getString(R.string.status_saving)
                        SaveStatus.Saved -> tvSaveStatus.text = getString(R.string.status_saved)
                        SaveStatus.SaveFailed -> tvSaveStatus.text = getString(R.string.status_save_failed)
                    }

                    tvWordCount.text = getString(R.string.stats_word_count, state.wordCount)
                    tvSessionAdded.text = getString(R.string.stats_session_added, state.sessionAdded)
                    tvSpeed.text = getString(R.string.stats_speed, state.speed)

                    applySettingsToEditor(state.settings)
                }
            }
        }

        viewLifecycleOwner.lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                viewModel.events.collectLatest { event ->
                    when (event) {
                        is EditorEvent.ToastMessage -> {
                            Toast.makeText(requireContext(), event.message, Toast.LENGTH_LONG).show()
                        }
                        is EditorEvent.ShowSaveFailedDialog -> {
                            showSaveFailedDialog(event.message)
                        }
                    }
                }
            }
        }
    }

    private fun applySettingsToEditor(settings: EditorSettingsState) {
        val tag = "WriterSettings"
        DiagnosticsLogger.d(tag, "applySettingsToEditor: fontSize=${settings.fontSize}, lineSpacing=${settings.lineSpacingMultiplier}, " +
            "autoIndent=${settings.autoIndentEnabled}/${settings.autoIndentWidth}, " +
            "typingAnim=${settings.typingAnimationEnabled}/${settings.typingAnimationDurationMs}ms, " +
            "smoothCursor=${settings.smoothCursorEnabled}/${settings.smoothCursorDurationMs}ms")

        // ── 自研写作区设置（唯一主路径） ──
        if (lastFontSize != settings.fontSize) {
            lastFontSize = settings.fontSize
            sujianEditorView.setFontSize(settings.fontSize)
            DiagnosticsLogger.d(tag, "  → fontSize applied to SujianEditorView: ${settings.fontSize}")
        }
        if (lastLineSpacing != settings.lineSpacingMultiplier) {
            lastLineSpacing = settings.lineSpacingMultiplier
            sujianEditorView.setLineSpacingMultiplier(settings.lineSpacingMultiplier)
            DiagnosticsLogger.d(tag, "  → lineSpacing applied to SujianEditorView: ${settings.lineSpacingMultiplier}")
        }
        if (lastTypingAnimEnabled != settings.typingAnimationEnabled || lastTypingAnimDuration != settings.typingAnimationDurationMs) {
            lastTypingAnimEnabled = settings.typingAnimationEnabled
            lastTypingAnimDuration = settings.typingAnimationDurationMs
            sujianEditorView.setTypingAnimationEnabled(settings.typingAnimationEnabled, settings.typingAnimationDurationMs)
            DiagnosticsLogger.d(tag, "  → typingAnimation applied to SujianEditorView: ${settings.typingAnimationEnabled}/${settings.typingAnimationDurationMs}ms")
        }
        if (lastSmoothCursorEnabled != settings.smoothCursorEnabled || lastSmoothCursorDuration != settings.smoothCursorDurationMs) {
            lastSmoothCursorEnabled = settings.smoothCursorEnabled
            lastSmoothCursorDuration = settings.smoothCursorDurationMs
            sujianEditorView.setSmoothCursorEnabled(settings.smoothCursorEnabled, settings.smoothCursorDurationMs)
            DiagnosticsLogger.d(tag, "  → smoothCursor applied to SujianEditorView: ${settings.smoothCursorEnabled}/${settings.smoothCursorDurationMs}ms")
        }
        // autoIndent 同步到自研写作区
        if (lastAutoIndentEnabled != settings.autoIndentEnabled || lastAutoIndentWidth != settings.autoIndentWidth) {
            lastAutoIndentEnabled = settings.autoIndentEnabled
            lastAutoIndentWidth = settings.autoIndentWidth
            sujianEditorView.setAutoIndent(settings.autoIndentEnabled, settings.autoIndentWidth)
            DiagnosticsLogger.d(tag, "  → autoIndent applied to SujianEditorView: ${settings.autoIndentEnabled}/${settings.autoIndentWidth}")
        }
        // 协调动画同步
        if (lastCoordinatedAnimEnabled != settings.coordinatedTextCursorAnimationEnabled) {
            lastCoordinatedAnimEnabled = settings.coordinatedTextCursorAnimationEnabled
            sujianEditorView.setCoordinatedAnimationEnabled(settings.coordinatedTextCursorAnimationEnabled)
            DiagnosticsLogger.d(tag, "  → coordinatedAnim applied to SujianEditorView: ${settings.coordinatedTextCursorAnimationEnabled}")
        }
        DiagnosticsLogger.d(tag, "applySettingsToEditor: SujianEditorView settings applied, typingAnim=${settings.typingAnimationEnabled}, smoothCursor=${settings.smoothCursorEnabled}")
    }

    // ── Save Failed Dialog ──

    private fun showSaveFailedDialog(message: String) {
        AlertDialog.Builder(requireContext())
            .setTitle(R.string.dialog_save_failed_title)
            .setMessage(message)
            .setPositiveButton(R.string.action_exit) { _, _ ->
                callback?.onBackRequested()
            }
            .setNegativeButton(R.string.action_retry) { _, _ ->
                lifecycleScope.launch {
                    val success = viewModel.requestSave().await()
                    if (success) {
                        callback?.onBackRequested()
                    }
                }
            }
            .show()
    }

    // ── Search and Replace ──

    private fun setupSearchAndReplace() {
        btnSearchClose.setOnClickListener {
            searchLayout.visibility = View.GONE
            clearHighlights()
        }

        etSearch.addTextChangedListener(object : android.text.TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                performSearch()
            }
            override fun afterTextChanged(s: android.text.Editable?) {}
        })

        btnSearchNext.setOnClickListener {
            if (searchResults.isNotEmpty()) {
                currentSearchIndex = (currentSearchIndex + 1) % searchResults.size
                focusSearchResult()
            }
        }

        // TODO: 替换功能待自研写作区 SujianEditorView 支持文本替换 API 后实现
        btnReplace.setOnClickListener {
            // 替换当前匹配项 — 暂未实现
        }

        btnReplaceAll.setOnClickListener {
            // 全部替换 — 暂未实现
        }
    }

    private fun performSearch() {
        clearHighlights()
        searchResults.clear()
        currentSearchIndex = -1

        if (searchLayout.visibility == View.GONE) return

        val searchStr = etSearch.text.toString()
        if (searchStr.isEmpty()) return

        val content = sujianEditorView.getText()
        var startIndex = content.indexOf(searchStr)

        while (startIndex >= 0) {
            val endIndex = startIndex + searchStr.length
            searchResults.add(Pair(startIndex, endIndex))
            startIndex = content.indexOf(searchStr, endIndex)
        }

        if (searchResults.isNotEmpty()) {
            currentSearchIndex = 0
            focusSearchResult()
        }
    }

    private fun focusSearchResult() {
        // TODO: 自研写作区搜索高亮和光标定位待 SujianEditorView 支持后实现
        // 目前仅记录当前搜索索引
    }

    private fun clearHighlights() {
        // TODO: 自研写作区搜索高亮清除待 SujianEditorView 支持后实现
        searchResults.clear()
        currentSearchIndex = -1
    }
}