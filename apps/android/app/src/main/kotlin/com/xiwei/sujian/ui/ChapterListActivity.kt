package com.xiwei.sujian.ui

import android.content.Intent
import android.os.Bundle
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import android.view.Menu
import android.view.MenuItem
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.ViewCompositionStrategy
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.google.android.material.appbar.MaterialToolbar
import com.google.android.material.floatingactionbutton.FloatingActionButton
import com.xiwei.sujian.R
import com.xiwei.sujian.editor.v2.compose.AnimatedTextField
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
import com.xiwei.sujian.data.WorkspaceRepository
import kotlinx.coroutines.*

/**
 * ChapterListActivity — 章节列表页面
 *
 * 展示作品的卷和章节列表，支持新建、重命名、删除和排序操作。
 *
 * ## 架构定位
 * - 从 MainActivity 跳转，展示指定作品的卷/章结构
 * - 通过 WorkspaceRepository 与 Rust Core 交互
 *
 * ## 职责边界
 * - **做**：卷/章列表展示、新建卷/章、重命名、删除、拖拽排序
 * - **不做**：章节内容编辑（由 EditorActivity 负责）
 *
 * ## 使用场景
 * - 用户点击作品卡片后进入
 * - 管理作品的卷和章节结构
 */
class ChapterListActivity : AppCompatActivity() {
    private lateinit var chapterRecyclerView: RecyclerView
    private lateinit var fabNewVolume: FloatingActionButton
    private lateinit var emptyStateLayout: View
    private lateinit var statsHeaderLayout: View
    private lateinit var tvStatsTotalWords: TextView
    private lateinit var tvStatsVolumes: TextView
    private lateinit var tvStatsChapters: TextView

    private lateinit var workspaceRepository: WorkspaceRepository
    private var projectId: String? = null
    private var listItems = mutableListOf<ListItem>()
    private lateinit var adapter: ChapterAdapter

    private sealed class ListItem {
        data class VolumeHeader(val volumeId: String, val volumeTitle: String) : ListItem()
        data class Chapter(val volumeId: String, val chapterId: String, val chapterTitle: String, val wordCount: Int) : ListItem()
        data class EmptyVolumeHint(val volumeId: String) : ListItem()
    }

    companion object {
        private const val TAG = "ChapterListActivity"
        private const val type_VOLUME_HEADER = 0
        private const val type_CHAPTER = 1
        private const val type_EMPTY_HINT = 2
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_chapter_list)

        window.decorView.post {
            UiFontUtil.applySansSerifFallback(window.decorView.rootView)
        }

        val toolbar: MaterialToolbar = findViewById(R.id.toolbar)
        setSupportActionBar(toolbar)
        toolbar.setNavigationOnClickListener { finish() }

        chapterRecyclerView = findViewById(R.id.chapterRecyclerView)
        fabNewVolume = findViewById(R.id.fabNewVolume)
        emptyStateLayout = findViewById(R.id.emptyStateLayout)
        statsHeaderLayout = findViewById(R.id.statsHeaderLayout)
        tvStatsTotalWords = findViewById(R.id.tvStatsTotalWords)
        tvStatsVolumes = findViewById(R.id.tvStatsVolumes)
        tvStatsChapters = findViewById(R.id.tvStatsChapters)

        workspaceRepository = WorkspaceRepository(this)

        projectId = intent.getStringExtra("PROJECT_ID")
        val projectTitle = intent.getStringExtra("PROJECT_TITLE")

        if (projectId == null) {
            finish()
            return
        }

        supportActionBar?.title = projectTitle ?: getString(R.string.title_projects)

        adapter = ChapterAdapter()
        chapterRecyclerView.layoutManager = LinearLayoutManager(this)
        chapterRecyclerView.adapter = adapter

        loadChapters()

        fabNewVolume.setOnClickListener {
            showNewVolumeDialog()
        }

        // ChapterListActivity 无底栏，FAB 只需 24dp 间距
        // 使用 WindowInsetsListener 确保 insets 到达后再调整 FAB 位置
        val density = resources.displayMetrics.density
        val rootView = window.decorView.findViewById<View>(android.R.id.content) ?: window.decorView
        androidx.core.view.ViewCompat.setOnApplyWindowInsetsListener(rootView) { view, insets ->
            val safeBottomInset = insets.getInsets(androidx.core.view.WindowInsetsCompat.Type.systemBars()).bottom
            FabPlacementHelper.adjustFabBottomMargin(fabNewVolume, hasBottomNav = false, bottomNavHeight = 0, safeBottomInset = safeBottomInset, density = density)
            insets
        }
    }

    override fun onCreateOptionsMenu(menu: Menu): Boolean {
        menuInflater.inflate(R.menu.menu_chapter_list, menu)
        return true
    }

    override fun onOptionsItemSelected(item: MenuItem): Boolean {
        return super.onOptionsItemSelected(item)
    }

    override fun onResume() {
        super.onResume()
        loadChapters()
        loadProjectStats()
    }

    private fun loadProjectStats() {
        val pid = projectId ?: return
        CoroutineScope(Dispatchers.IO).launch {
            try {
                val stats = workspaceRepository.getProjectStats(pid)
                withContext(Dispatchers.Main) {
                    statsHeaderLayout.visibility = View.VISIBLE
                    tvStatsTotalWords.text = getString(R.string.stats_total_words, stats.totalWordCount)
                    tvStatsVolumes.text = getString(R.string.stats_volumes, stats.volumeCount)
                    tvStatsChapters.text = getString(R.string.stats_chapters, stats.chapterCount)
                }
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "Failed to load project stats", e)
            }
        }
    }

    private fun loadChapters() {
        val pid = projectId ?: return
        listItems.clear()

        ErrorUtil.safeRun(this) {
            val volumes = workspaceRepository.getVolumes(pid)

            for (volume in volumes) {
                listItems.add(ListItem.VolumeHeader(volume.id, volume.title))
                val chapters = workspaceRepository.getChapters(pid, volume.id)
                if (chapters.isEmpty()) {
                    listItems.add(ListItem.EmptyVolumeHint(volume.id))
                } else {
                    for (chapter in chapters) {
                        listItems.add(
                            ListItem.Chapter(
                                volumeId = volume.id,
                                chapterId = chapter.id,
                                chapterTitle = chapter.title,
                                wordCount = chapter.wordCount
                            )
                        )
                    }
                }
            }
        }

        if (listItems.isEmpty()) {
            chapterRecyclerView.visibility = View.GONE
            emptyStateLayout.visibility = View.VISIBLE
        } else {
            chapterRecyclerView.visibility = View.VISIBLE
            emptyStateLayout.visibility = View.GONE
            adapter.notifyDataSetChanged()
        }
    }

    private fun showNewVolumeDialog() {
        val pid = projectId ?: return
        showComposeTextDialog(
            title = getString(R.string.dialog_new_volume_title),
            hint = getString(R.string.hint_volume_title),
            initialValue = ""
        ) { title ->
            if (title.isNotEmpty()) {
                ErrorUtil.safeRun(this) {
                    workspaceRepository.createVolume(pid, title)
                    loadChapters()
                    loadProjectStats()
                }
            }
        }
    }

    private fun showNewChapterDialog(volumeId: String) {
        val pid = projectId ?: return
        val chapterCount = listItems.count { it is ListItem.Chapter && it.volumeId == volumeId }
        val defaultTitle = getString(R.string.default_chapter_name_format, chapterCount + 1)

        showComposeTextDialog(
            title = getString(R.string.dialog_new_chapter_title),
            hint = getString(R.string.hint_chapter_title),
            initialValue = defaultTitle
        ) { title ->
            ErrorUtil.safeRun(this) {
                workspaceRepository.createChapter(pid, volumeId, title)
                loadChapters()
                loadProjectStats()
            }
        }
    }


    private fun showVolumeMenu(view: View, volumeId: String, volumeTitle: String) {
        val pid = projectId ?: return
        val popup = android.widget.PopupMenu(this, view)
        popup.menu.add(0, 1, 0, getString(R.string.action_rename))
        popup.menu.add(0, 2, 0, getString(R.string.action_delete))

        val volumes = listItems.filterIsInstance<ListItem.VolumeHeader>()
        val index = volumes.indexOfFirst { it.volumeId == volumeId }

        if (index > 0) {
            popup.menu.add(0, 3, 0, getString(R.string.action_move_up))
        }
        if (index >= 0 && index < volumes.size - 1) {
            popup.menu.add(0, 4, 0, getString(R.string.action_move_down))
        }

        popup.setOnMenuItemClickListener { item ->
            when (item.itemId) {
                1 -> {
                    showRenameVolumeDialog(volumeId, volumeTitle)
                    true
                }
                2 -> {
                    showDeleteVolumeDialog(volumeId, volumeTitle)
                    true
                }
                3 -> {
                    moveVolumeUp(volumeId)
                    true
                }
                4 -> {
                    moveVolumeDown(volumeId)
                    true
                }
                else -> false
            }
        }
        popup.show()
    }

    private fun showRenameVolumeDialog(volumeId: String, currentTitle: String) {
        val pid = projectId ?: return
        showComposeTextDialog(
            title = getString(R.string.action_rename),
            hint = getString(R.string.hint_volume_title),
            initialValue = currentTitle
        ) { newTitle ->
            if (newTitle.isNotEmpty() && newTitle != currentTitle) {
                ErrorUtil.safeRun(this) {
                    workspaceRepository.renameVolume(pid, volumeId, newTitle)
                    loadChapters()
                }
            }
        }
    }

    private fun showDeleteVolumeDialog(volumeId: String, title: String) {
        val pid = projectId ?: return
        AlertDialog.Builder(this)
            .setTitle(R.string.confirm_delete_volume)
            .setMessage(getString(R.string.confirm_delete_volume_message, title, getString(R.string.warning_delete_volume)))
            .setPositiveButton(R.string.action_delete) { _, _ ->
                ErrorUtil.safeRun(this) {
                    workspaceRepository.deleteVolume(pid, volumeId)
                    loadChapters()
                    loadProjectStats()
                }
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }

    private fun moveVolumeUp(volumeId: String) {
        val pid = projectId ?: return
        val volumes = listItems.filterIsInstance<ListItem.VolumeHeader>().map { it.volumeId }.toMutableList()
        val index = volumes.indexOf(volumeId)
        if (index <= 0) return
        val temp = volumes[index]
        volumes[index] = volumes[index - 1]
        volumes[index - 1] = temp

        ErrorUtil.safeRun(this) {
            workspaceRepository.reorderVolumes(pid, volumes)
            loadChapters()
        }
    }

    private fun moveVolumeDown(volumeId: String) {
        val pid = projectId ?: return
        val volumes = listItems.filterIsInstance<ListItem.VolumeHeader>().map { it.volumeId }.toMutableList()
        val index = volumes.indexOf(volumeId)
        if (index == -1 || index >= volumes.size - 1) return
        val temp = volumes[index]
        volumes[index] = volumes[index + 1]
        volumes[index + 1] = temp

        ErrorUtil.safeRun(this) {
            workspaceRepository.reorderVolumes(pid, volumes)
            loadChapters()
        }
    }

    private fun showChapterMenu(view: View, volumeId: String, chapterId: String, chapterTitle: String) {
        val pid = projectId ?: return
        val popup = android.widget.PopupMenu(this, view)
        popup.menu.add(0, 1, 0, getString(R.string.action_rename))
        popup.menu.add(0, 2, 0, getString(R.string.action_delete))

        val chapters = listItems.filterIsInstance<ListItem.Chapter>().filter { it.volumeId == volumeId }
        val index = chapters.indexOfFirst { it.chapterId == chapterId }

        if (index > 0) {
            popup.menu.add(0, 3, 0, getString(R.string.action_move_up))
        }
        if (index >= 0 && index < chapters.size - 1) {
            popup.menu.add(0, 4, 0, getString(R.string.action_move_down))
        }

        popup.setOnMenuItemClickListener { item ->
            when (item.itemId) {
                1 -> {
                    showRenameChapterDialog(volumeId, chapterId, chapterTitle)
                    true
                }
                2 -> {
                    showDeleteChapterDialog(volumeId, chapterId, chapterTitle)
                    true
                }
                3 -> {
                    moveChapterUp(volumeId, chapterId)
                    true
                }
                4 -> {
                    moveChapterDown(volumeId, chapterId)
                    true
                }
                else -> false
            }
        }
        popup.show()
    }

    private fun showRenameChapterDialog(volumeId: String, chapterId: String, currentTitle: String) {
        val pid = projectId ?: return
        showComposeTextDialog(
            title = getString(R.string.action_rename),
            hint = getString(R.string.hint_chapter_title),
            initialValue = currentTitle
        ) { newTitle ->
            if (newTitle.isNotEmpty() && newTitle != currentTitle) {
                ErrorUtil.safeRun(this) {
                    workspaceRepository.renameChapter(pid, volumeId, chapterId, newTitle)
                    loadChapters()
                }
            }
        }
    }

    private fun showDeleteChapterDialog(volumeId: String, chapterId: String, title: String) {
        val pid = projectId ?: return
        AlertDialog.Builder(this)
            .setTitle(R.string.confirm_delete_chapter)
            .setMessage(getString(R.string.confirm_delete_chapter_message, title))
            .setPositiveButton(R.string.action_delete) { _, _ ->
                ErrorUtil.safeRun(this) {
                    workspaceRepository.deleteChapter(pid, volumeId, chapterId)
                    loadChapters()
                    loadProjectStats()
                }
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }

    private fun showComposeTextDialog(
        title: String,
        hint: String,
        initialValue: String,
        onConfirm: (String) -> Unit
    ) {
        val composeView = ComposeView(this).apply {
            setViewCompositionStrategy(ViewCompositionStrategy.DisposeOnViewTreeLifecycleDestroyed)
        }
        var text by mutableStateOf(initialValue)
        var dialog: AlertDialog? = null

        composeView.setContent {
            androidx.compose.material3.AlertDialog(
                onDismissRequest = { dialog?.dismiss() },
                title = { Text(title) },
                text = {
                    AnimatedTextField(
                        targetId = "chapter-list-dialog:${title.hashCode()}",
                        value = text,
                        onValueChange = { text = it },
                        onCommit = {
                            onConfirm(it.trim())
                            dialog?.dismiss()
                        },
                        modifier = Modifier.fillMaxWidth(),
                        profile = TextEditorProfile.ShortTitle,
                        placeholder = { Text(hint) },
                        singleLine = true
                    )
                },
                confirmButton = {
                    TextButton(onClick = {
                        onConfirm(text.trim())
                        dialog?.dismiss()
                    }) {
                        Text(getString(R.string.action_ok))
                    }
                },
                dismissButton = {
                    TextButton(onClick = { dialog?.dismiss() }) {
                        Text(getString(R.string.action_cancel))
                    }
                }
            )
        }

        dialog = AlertDialog.Builder(this)
            .setView(composeView)
            .setCancelable(true)
            .create()
        dialog.show()
    }

    private fun moveChapterUp(volumeId: String, chapterId: String) {
        val pid = projectId ?: return
        val chapters = listItems.filterIsInstance<ListItem.Chapter>().filter { it.volumeId == volumeId }.map { it.chapterId }.toMutableList()
        val index = chapters.indexOf(chapterId)
        if (index <= 0) return
        val temp = chapters[index]
        chapters[index] = chapters[index - 1]
        chapters[index - 1] = temp

        ErrorUtil.safeRun(this) {
            workspaceRepository.reorderChapters(pid, volumeId, chapters)
            loadChapters()
        }
    }

    private fun moveChapterDown(volumeId: String, chapterId: String) {
        val pid = projectId ?: return
        val chapters = listItems.filterIsInstance<ListItem.Chapter>().filter { it.volumeId == volumeId }.map { it.chapterId }.toMutableList()
        val index = chapters.indexOf(chapterId)
        if (index == -1 || index >= chapters.size - 1) return
        val temp = chapters[index]
        chapters[index] = chapters[index + 1]
        chapters[index + 1] = temp

        ErrorUtil.safeRun(this) {
            workspaceRepository.reorderChapters(pid, volumeId, chapters)
            loadChapters()
        }
    }

    private inner class ChapterAdapter : RecyclerView.Adapter<RecyclerView.ViewHolder>() {

        inner class VolumeHeaderViewHolder(itemView: View) : RecyclerView.ViewHolder(itemView) {
            val tvVolumeTitle: TextView = itemView.findViewById(R.id.tvVolumeTitle)
            val btnAddChapter: View = itemView.findViewById(R.id.btnAddChapter)
            val btnMoreVolume: android.widget.ImageButton = itemView.findViewById(R.id.btnMoreVolume)

            init {
                itemView.isHapticFeedbackEnabled = false
                btnAddChapter.isHapticFeedbackEnabled = false
                btnMoreVolume.isHapticFeedbackEnabled = false

                btnMoreVolume.setOnClickListener {
                    val pos = adapterPosition
                    if (pos != RecyclerView.NO_POSITION) {
                        val item = listItems[pos] as? ListItem.VolumeHeader ?: return@setOnClickListener
                        showVolumeMenu(btnMoreVolume, item.volumeId, item.volumeTitle)
                    }
                }

                btnAddChapter.setOnClickListener {
                    val pos = adapterPosition
                    if (pos != RecyclerView.NO_POSITION) {
                        val item = listItems[pos] as? ListItem.VolumeHeader ?: return@setOnClickListener
                        showNewChapterDialog(item.volumeId)
                    }
                }

                itemView.setOnLongClickListener {
                    val pos = adapterPosition
                    if (pos != RecyclerView.NO_POSITION) {
                        val item = listItems[pos] as? ListItem.VolumeHeader
                        if (item != null) {
                            showVolumeMenu(itemView, item.volumeId, item.volumeTitle)
                            return@setOnLongClickListener true
                        }
                    }
                    false
                }
            }
        }

        inner class ChapterViewHolder(itemView: View) : RecyclerView.ViewHolder(itemView) {
            val tvChapterTitle: TextView = itemView.findViewById(R.id.tvChapterTitle)
            val tvWordCount: TextView = itemView.findViewById(R.id.tvWordCount)
            val btnMoreChapter: android.widget.ImageButton = itemView.findViewById(R.id.btnMoreChapter)

            init {
                itemView.isHapticFeedbackEnabled = false
                btnMoreChapter.isHapticFeedbackEnabled = false

                btnMoreChapter.setOnClickListener {
                    val pos = adapterPosition
                    if (pos != RecyclerView.NO_POSITION) {
                        val item = listItems[pos] as? ListItem.Chapter ?: return@setOnClickListener
                        showChapterMenu(btnMoreChapter, item.volumeId, item.chapterId, item.chapterTitle)
                    }
                }

                itemView.setOnClickListener {
                    val pos = adapterPosition
                    if (pos != RecyclerView.NO_POSITION) {
                        val selectedItem = listItems[pos] as? ListItem.Chapter ?: return@setOnClickListener
                        val intent = Intent(this@ChapterListActivity, EditorActivity::class.java).apply {
                            putExtra("PROJECT_ID", projectId)
                            putExtra("VOLUME_ID", selectedItem.volumeId)
                            putExtra("CHAPTER_ID", selectedItem.chapterId)
                            putExtra("CHAPTER_TITLE", selectedItem.chapterTitle)
                        }
                        try {
                            startActivity(intent)
                        } catch (e: Exception) {
                            DiagnosticsLogger.e(TAG, "Failed to open editor", e)
                            android.widget.Toast.makeText(this@ChapterListActivity, getString(R.string.error_open_editor, e.message ?: ""), android.widget.Toast.LENGTH_LONG).show()
                        }
                    }
                }

                itemView.setOnLongClickListener {
                    val pos = adapterPosition
                    if (pos != RecyclerView.NO_POSITION) {
                        val item = listItems[pos] as? ListItem.Chapter
                        if (item != null) {
                            showChapterMenu(itemView, item.volumeId, item.chapterId, item.chapterTitle)
                            return@setOnLongClickListener true
                        }
                    }
                    false
                }
            }
        }

        inner class EmptyHintViewHolder(itemView: View) : RecyclerView.ViewHolder(itemView)

        override fun getItemViewType(position: Int): Int {
            return when (listItems[position]) {
                is ListItem.VolumeHeader -> type_VOLUME_HEADER
                is ListItem.Chapter -> type_CHAPTER
                is ListItem.EmptyVolumeHint -> type_EMPTY_HINT
            }
        }

        override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): RecyclerView.ViewHolder {
            return when (viewType) {
                type_VOLUME_HEADER -> {
                    val view = LayoutInflater.from(parent.context).inflate(R.layout.item_volume_header, parent, false)
                    VolumeHeaderViewHolder(view)
                }
                type_CHAPTER -> {
                    val view = LayoutInflater.from(parent.context).inflate(R.layout.item_chapter, parent, false)
                    ChapterViewHolder(view)
                }
                else -> {
                    val view = LayoutInflater.from(parent.context).inflate(R.layout.item_empty_hint, parent, false)
                    EmptyHintViewHolder(view)
                }
            }
        }

        override fun onBindViewHolder(holder: RecyclerView.ViewHolder, position: Int) {
            when (val item = listItems[position]) {
                is ListItem.VolumeHeader -> {
                    (holder as VolumeHeaderViewHolder).tvVolumeTitle.text = item.volumeTitle
                }
                is ListItem.Chapter -> {
                    val chapterHolder = holder as ChapterViewHolder
                    chapterHolder.tvChapterTitle.text = item.chapterTitle
                    chapterHolder.tvWordCount.text = getString(R.string.word_count_label, item.wordCount)
                }
                is ListItem.EmptyVolumeHint -> {
                    // Nothing to bind
                }
            }
        }

        override fun getItemCount() = listItems.size
    }
}
