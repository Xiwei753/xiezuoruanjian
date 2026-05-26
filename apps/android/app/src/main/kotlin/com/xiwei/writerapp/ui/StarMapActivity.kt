package com.xiwei.writerapp.ui

import android.graphics.Color
import android.os.Bundle
import android.widget.FrameLayout
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import com.google.android.material.appbar.MaterialToolbar
import com.xiwei.writerapp.data.NativeCoreBridge
import com.xiwei.writerapp.data.NativeResult
import com.xiwei.writerapp.model.StarMapData
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class StarMapActivity : AppCompatActivity() {

    private lateinit var canvasView: StarMapCanvasView
    private var starmapId: String = ""
    private var currentData: StarMapData? = null

    private val bridge by lazy { NativeCoreBridge(this) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        starmapId = intent.getStringExtra("STARMAP_ID") ?: ""
        val title = intent.getStringExtra("TITLE") ?: "星图"

        val layout = FrameLayout(this)

        val toolbar = MaterialToolbar(this).apply {
            setTitle(title)
            setTitleTextColor(Color.WHITE)
            setBackgroundColor(Color.parseColor("#1A1D23"))
            setNavigationIcon(androidx.appcompat.R.drawable.abc_ic_ab_back_material)
            setNavigationOnClickListener { finish() }
        }

        canvasView = StarMapCanvasView(this).apply {
            layoutParams = FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.MATCH_PARENT
            ).apply {
                topMargin = (56 * resources.displayMetrics.density).toInt() // Approx toolbar height
            }
            onLayoutChangedListener = {
                saveLayout()
            }
        }

        layout.addView(canvasView)
        layout.addView(toolbar, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, (56 * resources.displayMetrics.density).toInt()))

        setContentView(layout)

        loadGraph()
    }

    private fun loadGraph() {
        if (starmapId.isEmpty()) return
        CoroutineScope(Dispatchers.IO).launch {
            val result = bridge.getStarmapGraph(starmapId)
            withContext(Dispatchers.Main) {
                when (result) {
                    is NativeResult.Success -> {
                        currentData = result.data
                        canvasView.setData(result.data)
                    }
                    is NativeResult.Error -> {
                        Toast.makeText(this@StarMapActivity, "Failed to load: ${result.message}", Toast.LENGTH_LONG).show()
                    }
                    else -> {}
                }
            }
        }
    }

    private fun saveLayout() {
        val data = canvasView.getData() ?: return
        CoroutineScope(Dispatchers.IO).launch {
            bridge.saveStarmapLayout(starmapId, data.layout)
        }
    }
    }
