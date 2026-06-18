package com.xiwei.sujian.ui

import android.animation.ValueAnimator
import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.os.SystemClock
import android.util.AttributeSet
import android.view.Choreographer
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapMotionPolicyData
import com.xiwei.sujian.model.StarMapNodeKind
import com.xiwei.sujian.model.StarMapViewportData

/**
 * StarMapCanvasView — 星图画布自定义 View
 *
 * 使用 Canvas 绘制星图节点和连线，支持平移和缩放交互。
 * 连线几何和节点命中测试由 Rust Core 计算；节点样式、文字和颜色仍是 Android 端绘制逻辑。
 *
 * ## 架构定位
 * - StarMapController → StarMapCanvasView → Canvas 绘制
 *
 * ## 职责边界
 * - **做**：星图可视化渲染、节点拖拽、平移/缩放交互、idle wobble / drag lift / settle 动画
 * - **不做**：数据管理（由 StarMapController 负责）
 *
 * ## 动画架构
 * - idle wobble：Choreographer 帧回调驱动，视觉偏移不写入 layout.x/y
 * - drag lift：拖动时 scale 1.04 + shadow 加强
 * - settle：ACTION_UP 后 ValueAnimator 220ms 归位
 * - reduceMotion：关闭所有动画
 *
 * ## 使用场景
 * - MainActivity 星图标签页的可视化展示
 * - 用户拖拽节点调整布局
 */
class StarMapCanvasView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0
) : View(context, attrs, defStyleAttr) {

    private var data: StarMapData? = null

    private val paintNodeBg = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.parseColor("#1A1D23") }
    private val paintNodeBorder = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.parseColor("#2A2E36")
        style = Paint.Style.STROKE
        strokeWidth = 2f
    }
    private val paintHeader = Paint(Paint.ANTI_ALIAS_FLAG)

    private val paintTitleText = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.parseColor("#E2E4E9")
        textSize = 36f
        textAlign = Paint.Align.CENTER
    }

    private val paintKindText = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.WHITE
        textSize = 28f
        typeface = Typeface.DEFAULT_BOLD
        textAlign = Paint.Align.CENTER
    }

    private val paintEdge = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.parseColor("#4C566A")
        strokeWidth = 4f
        style = Paint.Style.STROKE
    }

    var panX = 0f
    var panY = 0f
    var zoom = 1f
    var lastTouchX = 0f
    var lastTouchY = 0f
    var draggingNodeId: String? = null
    var onNodeDragListener: ((String, Float, Float) -> Unit)? = null
    var onNodeHitTestListener: ((Float, Float) -> String?)? = null
    var onViewportChangedListener: ((StarMapViewportData) -> Unit)? = null
    var onLayoutSavedListener: (() -> Unit)? = null
    private var viewportDirty = false

    // ── 动画状态 ──
    private var motionPolicy = StarMapMotionPolicyData()
    private var animationStartMs = SystemClock.uptimeMillis()

    /** settle 动画：拖动释放后从视觉偏移归位到 layout.x/y */
    private var settleAnimator: ValueAnimator? = null

    /** settle 动画期间记录的节点视觉偏移（dx, dy） */
    private var settleOffsetMap = mutableMapOf<String, VisualOffset>()

    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (motionPolicy.enabled && data != null) {
                invalidate()
                Choreographer.getInstance().postFrameCallback(this)
            }
        }
    }

    /** 视觉偏移数据：idle wobble 的 dx/dy 和 scale */
    private data class VisualOffset(val dx: Float, val dy: Float, val scale: Float)

    private val scaleGestureDetector = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScale(detector: ScaleGestureDetector): Boolean {
            val oldZoom = zoom
            val newZoom = (zoom * detector.scaleFactor).coerceIn(0.35f, 3.0f)
            if (newZoom == oldZoom) return false

            val focusX = detector.focusX
            val focusY = detector.focusY
            val graphFocusX = (focusX - panX) / oldZoom
            val graphFocusY = (focusY - panY) / oldZoom
            zoom = newZoom
            panX = focusX - graphFocusX * zoom
            panY = focusY - graphFocusY * zoom
            viewportDirty = true
            invalidate()
            return true
        }

        override fun onScaleEnd(detector: ScaleGestureDetector) {
            notifyViewportChangedIfNeeded()
        }
    })

    fun getData(): StarMapData? = data

    fun setData(newData: StarMapData) {
        this.data = newData
        invalidate()
    }

    fun setViewport(viewport: StarMapViewportData) {
        zoom = viewport.scale.coerceIn(0.35f, 3.0f)
        panX = viewport.offsetX
        panY = viewport.offsetY
        viewportDirty = false
        invalidate()
    }

    fun currentViewport(): StarMapViewportData = StarMapViewportData(
        scale = zoom,
        offsetX = panX,
        offsetY = panY,
        width = width.toFloat(),
        height = height.toFloat()
    )

    private fun notifyViewportChangedIfNeeded() {
        if (!viewportDirty) return
        viewportDirty = false
        onViewportChangedListener?.invoke(currentViewport())
    }

    private fun getKindColor(kind: StarMapNodeKind): Int {
        return when (kind) {
            StarMapNodeKind.Chapter -> Color.parseColor("#4CAF50")
            StarMapNodeKind.Character -> Color.parseColor("#2196F3")
            StarMapNodeKind.Location -> Color.parseColor("#FF9800")
            StarMapNodeKind.Event -> Color.parseColor("#F44336")
            StarMapNodeKind.Concept -> Color.parseColor("#9C27B0")
            else -> Color.parseColor("#606470")
        }
    }

    private fun getKindString(kind: StarMapNodeKind): String {
        return when (kind) {
            StarMapNodeKind.Note -> "Note"
            StarMapNodeKind.Chapter -> "Chapter"
            StarMapNodeKind.Character -> "Character"
            StarMapNodeKind.Location -> "Location"
            StarMapNodeKind.Event -> "Event"
            StarMapNodeKind.Concept -> "Concept"
            StarMapNodeKind.Theme -> "Theme"
            StarMapNodeKind.Item -> "Item"
            StarMapNodeKind.Organization -> "Organization"
            StarMapNodeKind.Timeline -> "Timeline"
            StarMapNodeKind.Plot -> "Plot"
            StarMapNodeKind.Foreshadowing -> "Foreshadowing"
            StarMapNodeKind.Custom -> "Custom"
        }
    }

    /**
     * 计算节点的视觉偏移（idle wobble），不修改 layout.x/y。
     * 使用 nodeId hash 算稳定 phase，不用随机数。
     */
    private fun computeVisualOffset(nodeId: String, isDragging: Boolean): VisualOffset {
        if (!motionPolicy.enabled || !motionPolicy.idleWobbleEnabled || isDragging || motionPolicy.reduceMotion) {
            return VisualOffset(0f, 0f, 1f)
        }

        // 稳定 phase：用 nodeId hash 算
        val phase = (nodeId.hashCode().toLong() and 0xFFFFFFFFL) % 360
        val phaseRad = Math.toRadians(phase.toDouble())

        val elapsed = (SystemClock.uptimeMillis() - animationStartMs).toFloat()
        val periodMs = motionPolicy.idlePeriodMs.toFloat()
        val amplitude = motionPolicy.idleAmplitudeVp * resources.displayMetrics.density

        val wobbleX = (Math.sin((elapsed / periodMs) * 2 * Math.PI + phaseRad) * amplitude).toFloat()
        val wobbleY = (Math.cos((elapsed / periodMs) * 0.7 * 2 * Math.PI + phaseRad) * amplitude * 0.6).toFloat()

        return VisualOffset(wobbleX, wobbleY, 1f)
    }

    /**
     * 启动 settle 动画：拖动释放后从当前视觉偏移平滑归位到 layout.x/y。
     */
    private fun startSettleAnimation(nodeId: String, startDx: Float, startDy: Float) {
        settleAnimator?.cancel()

        settleOffsetMap[nodeId] = VisualOffset(startDx, startDy, motionPolicy.dragLiftScale)

        val animator = ValueAnimator.ofFloat(1f, 0f)
        animator.duration = motionPolicy.settleDurationMs.toLong()
        animator.addUpdateListener { animation ->
            val fraction = animation.animatedValue as Float
            val dx = startDx * fraction
            val dy = startDy * fraction
            val scale = 1f + (motionPolicy.dragLiftScale - 1f) * fraction
            settleOffsetMap[nodeId] = VisualOffset(dx, dy, scale)
            invalidate()
        }
        animator.addListener(object : android.animation.AnimatorListenerAdapter() {
            override fun onAnimationEnd(animation: android.animation.Animator) {
                settleOffsetMap.remove(nodeId)
                invalidate()
            }
        })
        settleAnimator = animator
        animator.start()
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        canvas.drawColor(Color.parseColor("#111318"))

        val currentData = data ?: return

        canvas.save()
        canvas.translate(panX, panY)
        canvas.scale(zoom, zoom)

        // ── 可见区域裁剪 ──
        val visibleLeft = -panX / zoom - 100
        val visibleTop = -panY / zoom - 100
        val visibleRight = visibleLeft + width / zoom + 200
        val visibleBottom = visibleTop + height / zoom + 200

        for (edge in currentData.edgeRenders) {
            // 通过 edgeId 找到 graph edge，获取 from/to nodeId，计算 visual offset
            val graphEdge = currentData.graph.edges.find { it.id == edge.edgeId }
            val fromNodeId = graphEdge?.from
            val toNodeId = graphEdge?.to

            // source 端 visual offset
            val fromVisual = fromNodeId?.let { nid ->
                val isDragging = draggingNodeId == nid
                settleOffsetMap[nid] ?: computeVisualOffset(nid, isDragging)
            } ?: VisualOffset(0f, 0f, 1f)

            // target 端 visual offset
            val toVisual = toNodeId?.let { nid ->
                val isDragging = draggingNodeId == nid
                settleOffsetMap[nid] ?: computeVisualOffset(nid, isDragging)
            } ?: VisualOffset(0f, 0f, 1f)

            // 修正坐标：edge 基础坐标 + 端点 visual offset
            val sx = edge.startX + fromVisual.dx
            val sy = edge.startY + fromVisual.dy
            val ex = edge.endX + toVisual.dx
            val ey = edge.endY + toVisual.dy

            canvas.drawLine(sx, sy, ex, ey, paintEdge)

            // 箭头也需要偏移（to 端偏移）
            val atx = edge.arrowTipX + toVisual.dx
            val aty = edge.arrowTipY + toVisual.dy
            val alx = edge.arrowLeftX + toVisual.dx
            val aly = edge.arrowLeftY + toVisual.dy
            val arx = edge.arrowRightX + toVisual.dx
            val ary = edge.arrowRightY + toVisual.dy

            canvas.drawLine(atx, aty, alx, aly, paintEdge)
            canvas.drawLine(atx, aty, arx, ary, paintEdge)
        }

        for (node in currentData.graph.nodes) {
            val layout = currentData.layout.nodes.find { it.nodeId == node.id }
            if (layout != null) {
                // 粗略裁剪：跳过不可见节点
                if (layout.x + layout.width < visibleLeft || layout.x > visibleRight ||
                    layout.y + layout.height < visibleTop || layout.y > visibleBottom) {
                    continue
                }

                val isDragging = draggingNodeId == node.id

                // settle 动画中的节点使用 settle 偏移，否则用 idle wobble
                val settleOffset = settleOffsetMap[node.id]
                val visual = if (settleOffset != null) {
                    settleOffset
                } else {
                    computeVisualOffset(node.id, isDragging)
                }

                val scale = if (isDragging) motionPolicy.dragLiftScale else visual.scale

                val vx = layout.x + visual.dx
                val vy = layout.y + visual.dy
                val vw = layout.width * scale
                val vh = layout.height * scale

                // 拖动时阴影加强
                if (isDragging) {
                    paintNodeBg.setShadowLayer(motionPolicy.dragShadowBoost, 0f, 4f, Color.parseColor("#66000000"))
                } else {
                    paintNodeBg.clearShadowLayer()
                }

                val rect = RectF(vx, vy, vx + vw, vy + vh)

                // Draw node background
                canvas.drawRoundRect(rect, 16f, 16f, paintNodeBg)

                // Draw border
                canvas.drawRoundRect(rect, 16f, 16f, paintNodeBorder)

                // Draw header strip
                paintHeader.color = getKindColor(node.kind)
                val headerRect = RectF(vx + 16f, vy + 16f, vx + vw - 16f, vy + 16f + 48f)
                canvas.drawRoundRect(headerRect, 8f, 8f, paintHeader)

                // Draw kind text
                canvas.drawText(
                    getKindString(node.kind),
                    vx + vw / 2,
                    vy + 16f + 34f,
                    paintKindText
                )

                // Draw title text
                canvas.drawText(
                    node.title,
                    vx + vw / 2,
                    vy + vh / 2 + 32f,
                    paintTitleText
                )
            }
        }

        canvas.restore()
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        val x = event.x
        val y = event.y
        scaleGestureDetector.onTouchEvent(event)

        if (event.pointerCount > 1 || scaleGestureDetector.isInProgress) {
            draggingNodeId = null
            lastTouchX = x
            lastTouchY = y
            return true
        }

        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                lastTouchX = x
                lastTouchY = y

                val graphX = (x - panX) / zoom
                val graphY = (y - panY) / zoom

                draggingNodeId = onNodeHitTestListener?.invoke(graphX, graphY)
            }
            MotionEvent.ACTION_MOVE -> {
                val dx = x - lastTouchX
                val dy = y - lastTouchY

                if (draggingNodeId != null) {
                    onNodeDragListener?.invoke(draggingNodeId!!, dx / zoom, dy / zoom)
                } else {
                    panX += dx
                    panY += dy
                    viewportDirty = true
                }

                lastTouchX = x
                lastTouchY = y
                invalidate()
            }
            MotionEvent.ACTION_UP -> {
                if (draggingNodeId != null) {
                    onLayoutSavedListener?.invoke()

                    // 启动 settle 动画：从当前视觉偏移归位
                    if (motionPolicy.enabled && !motionPolicy.reduceMotion) {
                        val visual = computeVisualOffset(draggingNodeId!!, true)
                        startSettleAnimation(draggingNodeId!!, visual.dx, visual.dy)
                    }
                }
                draggingNodeId = null
                notifyViewportChangedIfNeeded()
            }
        }
        return true
    }

    // ── 生命周期管理 ──

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        if (motionPolicy.enabled) {
            Choreographer.getInstance().postFrameCallback(frameCallback)
        }
    }

    override fun onDetachedFromWindow() {
        super.onDetachedFromWindow()
        Choreographer.getInstance().removeFrameCallback(frameCallback)
        settleAnimator?.cancel()
    }

    /**
     * 设置动画策略参数。
     * 由 StarMapController 从 Core 层获取策略后调用。
     */
    fun setMotionPolicy(policy: StarMapMotionPolicyData) {
        this.motionPolicy = policy
        if (policy.enabled && isAttachedToWindow) {
            Choreographer.getInstance().postFrameCallback(frameCallback)
        } else {
            Choreographer.getInstance().removeFrameCallback(frameCallback)
        }
        invalidate()
    }
}
