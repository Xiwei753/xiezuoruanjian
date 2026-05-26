package com.xiwei.writerapp.ui

import android.content.Intent
import android.os.Bundle
import android.view.Menu
import android.view.MenuItem
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.TextView
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.google.android.material.appbar.MaterialToolbar
import com.google.android.material.floatingactionbutton.ExtendedFloatingActionButton
import com.xiwei.writerapp.R
import com.xiwei.writerapp.data.WorkspaceRepository
import kotlinx.coroutines.*

class ChapterListActivity : AppCompatActivity() {
    private lateinit var chapterRecyclerView: RecyclerView
    private lateinit var fabNewVolume: ExtendedFloatingActionButton
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
                    tvStatsTotalWords.text = "总字数: ${stats.totalWordCount}"
                    tvStatsVolumes.text = "卷: ${stats.volumeCount}"
                    tvStatsChapters.text = "章: ${stats.chapterCount}"
                }
            } catch (e: Exception) {
                e.printStackTrace()
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

        val editText = EditText(this)
        editText.hint = getString(R.string.hint_volume_title)
        editText.setPadding(48, 48, 48, 48)

        AlertDialog.Builder(this)
            .setTitle(R.string.dialog_new_volume_title)
            .setView(editText)
            .setPositiveButton(R.string.action_create) { _, _ ->
                val title = editText.text.toString().trim()
                if (title.isNotEmpty()) {
                    ErrorUtil.safeRun(this) {
                        workspaceRepository.createVolume(pid, title)
                        loadChapters()
                        loadProjectStats()
                    }
                }
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }

    private fun showNewChapterDialog(volumeId: String) {
        val pid = projectId ?: return

        val editText = EditText(this)
        editText.hint = getString(R.string.hint_chapter_title)
        editText.setPadding(48, 48, 48, 48)

        AlertDialog.Builder(this)
            .setTitle(R.string.dialog_new_chapter_title)
            .setView(editText)
            .setPositiveButton(R.string.action_create) { _, _ ->
                val title = editText.text.toString().trim()
                if (title.isNotEmpty()) {
                    ErrorUtil.safeRun(this) {
                        workspaceRepository.createChapter(pid, volumeId, title)
                        loadChapters()
                        loadProjectStats()
                    }
                }
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
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
        val editText = android.widget.EditText(this)
        editText.setText(currentTitle)
        editText.setSelection(currentTitle.length)
        editText.hint = getString(R.string.hint_volume_title)
        editText.setPadding(48, 48, 48, 48)

        AlertDialog.Builder(this)
            .setTitle(R.string.action_rename)
            .setView(editText)
            .setPositiveButton(R.string.action_ok) { _, _ ->
                val newTitle = editText.text.toString().trim()
                if (newTitle.isNotEmpty() && newTitle != currentTitle) {
                    ErrorUtil.safeRun(this) {
                        workspaceRepository.renameVolume(pid, volumeId, newTitle)
                        loadChapters()
                    }
                }
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }

    private fun showDeleteVolumeDialog(volumeId: String, title: String) {
        val pid = projectId ?: return
        AlertDialog.Builder(this)
            .setTitle(R.string.confirm_delete_volume)
            .setMessage("确定要删除分卷 \"${title}\" 吗？\n" + getString(R.string.warning_delete_volume))
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
        val editText = android.widget.EditText(this)
        editText.setText(currentTitle)
        editText.setSelection(currentTitle.length)
        editText.hint = getString(R.string.hint_chapter_title)
        editText.setPadding(48, 48, 48, 48)

        AlertDialog.Builder(this)
            .setTitle(R.string.action_rename)
            .setView(editText)
            .setPositiveButton(R.string.action_ok) { _, _ ->
                val newTitle = editText.text.toString().trim()
                if (newTitle.isNotEmpty() && newTitle != currentTitle) {
                    ErrorUtil.safeRun(this) {
                        workspaceRepository.renameChapter(pid, volumeId, chapterId, newTitle)
                        loadChapters()
                    }
                }
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }

    private fun showDeleteChapterDialog(volumeId: String, chapterId: String, title: String) {
        val pid = projectId ?: return
        AlertDialog.Builder(this)
            .setTitle(R.string.confirm_delete_chapter)
            .setMessage("确定要删除章节 \"${title}\" 吗？")
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
                            e.printStackTrace()
                            android.widget.Toast.makeText(this@ChapterListActivity, "无法打开编辑器: ${e.message}", android.widget.Toast.LENGTH_LONG).show()
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
                    chapterHolder.tvWordCount.text = "字数: ${item.wordCount}"
                }
                is ListItem.EmptyVolumeHint -> {
                    // Nothing to bind
                }
            }
        }

        override fun getItemCount() = listItems.size
    }
}
