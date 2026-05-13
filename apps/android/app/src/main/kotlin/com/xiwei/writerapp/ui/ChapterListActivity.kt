package com.xiwei.writerapp.ui

import android.content.Intent
import android.os.Bundle
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

class ChapterListActivity : AppCompatActivity() {
    private lateinit var chapterRecyclerView: RecyclerView
    private lateinit var fabNewChapter: ExtendedFloatingActionButton
    private lateinit var emptyStateLayout: View

    private lateinit var workspaceRepository: WorkspaceRepository
    private var projectId: String? = null
    private var listItems = mutableListOf<ChapterListItem>()
    private lateinit var adapter: ChapterAdapter

    private data class ChapterListItem(val volumeId: String, val volumeTitle: String, val chapterId: String, val chapterTitle: String)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_chapter_list)

        val toolbar: MaterialToolbar = findViewById(R.id.toolbar)
        setSupportActionBar(toolbar)
        toolbar.setNavigationOnClickListener { finish() }

        chapterRecyclerView = findViewById(R.id.chapterRecyclerView)
        fabNewChapter = findViewById(R.id.fabNewChapter)
        emptyStateLayout = findViewById(R.id.emptyStateLayout)

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

        fabNewChapter.setOnClickListener {
            showNewChapterDialog()
        }
    }

    override fun onResume() {
        super.onResume()
        loadChapters()
    }

    private fun loadChapters() {
        val pid = projectId ?: return
        listItems.clear()

        ErrorUtil.safeRun(this) {
            val volumes = workspaceRepository.getVolumes(pid)

            for (volume in volumes) {
                val chapters = workspaceRepository.getChapters(pid, volume.id)
                for (chapter in chapters) {
                    listItems.add(
                        ChapterListItem(
                            volumeId = volume.id,
                            volumeTitle = volume.title,
                            chapterId = chapter.id,
                            chapterTitle = chapter.title
                        )
                    )
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

    private fun showNewChapterDialog() {
        val pid = projectId ?: return

        val volumes = ErrorUtil.safeRun(this, emptyList()) {
            workspaceRepository.getVolumes(pid)
        }

        if (volumes.isEmpty()) {
            // Need at least one volume
            return
        }
        val defaultVolumeId = volumes.first().id

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
                        workspaceRepository.createChapter(pid, defaultVolumeId, title)
                        loadChapters()
                    }
                }
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }

    private inner class ChapterAdapter : RecyclerView.Adapter<ChapterAdapter.ChapterViewHolder>() {

        inner class ChapterViewHolder(itemView: View) : RecyclerView.ViewHolder(itemView) {
            val tvChapterTitle: TextView = itemView.findViewById(R.id.tvChapterTitle)
            val tvVolumeTitle: TextView = itemView.findViewById(R.id.tvVolumeTitle)

            init {
                itemView.setOnClickListener {
                    val selectedItem = listItems[adapterPosition]
                    val intent = Intent(this@ChapterListActivity, EditorActivity::class.java).apply {
                        putExtra("PROJECT_ID", projectId)
                        putExtra("VOLUME_ID", selectedItem.volumeId)
                        putExtra("CHAPTER_ID", selectedItem.chapterId)
                        putExtra("CHAPTER_TITLE", selectedItem.chapterTitle)
                    }
                    startActivity(intent)
                }
            }
        }

        override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ChapterViewHolder {
            val view = LayoutInflater.from(parent.context).inflate(R.layout.item_chapter, parent, false)
            return ChapterViewHolder(view)
        }

        override fun onBindViewHolder(holder: ChapterViewHolder, position: Int) {
            val item = listItems[position]
            holder.tvChapterTitle.text = item.chapterTitle
            holder.tvVolumeTitle.text = item.volumeTitle
        }

        override fun getItemCount() = listItems.size
    }
}
