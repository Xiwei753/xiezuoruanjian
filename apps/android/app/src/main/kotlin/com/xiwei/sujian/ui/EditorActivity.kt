package com.xiwei.sujian.ui

import android.os.Bundle
import androidx.activity.OnBackPressedCallback
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import com.google.android.material.appbar.MaterialToolbar
import com.xiwei.sujian.R
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

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        androidx.core.view.WindowCompat.setDecorFitsSystemWindows(window, false)
        setContentView(R.layout.activity_editor)

        window.decorView.post {
            UiFontUtil.applySansSerifFallback(window.decorView.rootView)
        }

        val mainLayout = findViewById<android.view.View>(R.id.editorCoordinatorLayout)
        val appBarLayout = findViewById<android.view.View>(R.id.appBarLayout)

        ViewCompat.setOnApplyWindowInsetsListener(mainLayout) { view, insets ->
            val systemBarsInsets = insets.getInsets(WindowInsetsCompat.Type.systemBars())
            appBarLayout.setPadding(0, systemBarsInsets.top, 0, 0)
            WindowInsetsCompat.CONSUMED
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
}
