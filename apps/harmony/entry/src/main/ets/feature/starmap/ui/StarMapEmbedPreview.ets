// ─── StarMapEmbedPreview.ets ───
// Deep Zoom 的 preview 档：Embed 圆壳内部的轻量缩略图（#821）。
//
// 它只回答一个问题："这顆子星图里面大概有什么"。所以：
// - 画在一张**固定尺寸**的 Canvas 上（DEFAULT_EMBED_DIAMETER 见方），
//   缩放由父层 Embed 外壳的 `.scale()` 承担，本组件自己永远不碰布局尺寸；
// - 子节点、子边、子 Embed 全部走和 interactive 层同一套只读几何
//   （buildFreeformLayout / buildEmbedLayoutNodes / computeAllEdgeRenders），
//   再用 StarMapViewport 的 computeContentBounds + computeFittedViewport 做一次
//   local fit，保证"里面有什么"在任何缩放下都留在圆里；
// - 子 Embed 同样是圆，和它们在 interactive 层、在父 Scene 里的几何一致，
//   不因为掉档就换成方形摘要卡；
// - 投影太小就不画文字：48vp 的圆上写标题只会糊成一团像素；
// - 不注册 Scene handle、不挂任何手势、整块 HitTestMode.None：
//   preview 是"看得见"，不是"点得动"。根 Scene 的两指 Pinch 必须完整穿过它，
//   交互责任属于 interactive 层（coverage ≥ 0.70 时）的 StarMapEmbedScene。
//
// #821 边界：档位只决定"画多少细节"，绝不改 Embed 的形状、world bounds 或
// authored position —— 本组件是只读渲染器，连 bridge 的写接口都不碰。

import hilog from '@ohos.hilog'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { StarMapGraph, StarMapEmbed, StarMapPathSegment } from '../../../corebridge/dto/StarMapDtos'
import { ThemeConfig } from '../../../ui/theme/HarmonyThemeAdapter'
import {
  PlatformLayoutNode,
  buildFreeformLayout,
  buildEmbedLayoutNodes,
  DEFAULT_EMBED_DIAMETER,
  DEFAULT_NODE_RADIUS
} from '../platform/StarMapLayout'
import {
  PlatformNodeRect,
  PlatformEdgeRender,
  deriveNodeRectFromLayoutNode,
  computeAllEdgeRenders
} from '../platform/StarMapGeometry'
import {
  computeContentBounds,
  computeEmbedInnerContentSafeSide,
  computeFittedViewport
} from '../platform/StarMapViewport'

/** 预览画布的边长：恒为本地 Embed 直径，不随父层缩放变化。 */
const PREVIEW_CANVAS_SIZE: number = DEFAULT_EMBED_DIAMETER

/** 内容区留白（vp，本地单位）：和 interactive 层 fitView 用同一份量级。 */
const PREVIEW_PADDING_VP: number = 6

/**
 * 文字最小可读阈值（本地单位下的节点边长）。
 * 低于它就不画标签：缩略图上的文字只会糊成色块，反而降低可辨识度。
 */
const PREVIEW_LABEL_MIN_SIDE_VP: number = 44

@Component
export struct StarMapEmbedPreview {
  // ─── @Prop：从父组件传入 ───
  /** 根星图 ID（解析子路径要用起点） */
  @Prop rootStarmapId: string = ''
  /** 当前 scene 的路径段（不含当前 Embed 的 segment） */
  @Prop scenePath: StarMapPathSegment[] = []
  /** 当前 Embed 实例数据 */
  @Prop embed: StarMapEmbed = {
    instanceId: '',
    targetStarmapId: '',
    label: null,
    position: { x: 0, y: 0 },
    hostPath: {
      starmapId: '', segments: [],
      target: {
        type: 'starmap', nodeId: null, anchorId: null,
        projectId: null, volumeId: null, chapterId: null,
        rangeStart: null, rangeEnd: null,
        entityType: null, entityId: null, uri: null
      }
    },
    provenance: {
      source: 'Human', sourceId: null,
      generatedBy: null, promptId: null,
      reviewStatus: 'Accepted', createdFromAnchor: null
    },
    createdAt: 0, updatedAt: 0
  }
  /** 主题配置（只读颜色） */
  @Prop themeConfig: ThemeConfig = {
    mode: 'light', colorSource: 'built_in',
    selectedPaletteId: '', selectedBuiltinThemeId: '',
    colors: {
      primary: '#000000', onPrimary: '#FFFFFF',
      primaryContainer: '#E0E0E0', onPrimaryContainer: '#1A1C2E',
      secondary: '#51606F', onSecondary: '#FFFFFF',
      secondaryContainer: '#D5E2F0', onSecondaryContainer: '#0D1E2C',
      tertiary: '#695779', onTertiary: '#FFFFFF',
      tertiaryContainer: '#F0DBFF', onTertiaryContainer: '#241533',
      error: '#BA1A1A', onError: '#FFFFFF',
      errorContainer: '#FFDAD6', onErrorContainer: '#410002',
      background: '#FDFCFF', onBackground: '#1A1C2E',
      surface: '#FDFCFF', onSurface: '#1A1C2E',
      surfaceVariant: '#DFE2EB', onSurfaceVariant: '#43474E',
      surfaceContainerLowest: '#FFFFFF', surfaceContainerLow: '#F7F8FA',
      surfaceContainer: '#F1F2F5', surfaceContainerHigh: '#EBEDF0',
      surfaceContainerHighest: '#E6E7EA', surfaceDim: '#DAD9DD',
      surfaceBright: '#FDFCFF',
      outline: '#73777F', outlineVariant: '#C3C7CF',
      inverseSurface: '#2F3033', inverseOnSurface: '#F1F0F4',
      inversePrimary: '#A8C8FF',
      scrim: '#000000',
      textPrimary: '#1A1C2E', textSecondary: '#43474E',
      textHint: '#73777F', success: '#2ECC71',
      divider: '#E0E0E0', overlay: '#80000000'
    },
    fontSize: 14, fontFamily: 'HarmonyOS Sans', lineHeight: 1.5
  }
  /** bridge：只用来读子星图 */
  bridge: IWriterCoreBridge | null = null

  // ─── @State ───
  /** 子星图 graph */
  @State childGraph: StarMapGraph | null = null
  /** resolver 解析出的最终 starmapId */
  @State finalStarmapId: string = ''
  /** 子节点矩形（本地画布坐标，未 fit） */
  @State childRects: PlatformNodeRect[] = []
  /** 子边渲染几何（本地画布坐标，未 fit） */
  @State edgeRenders: PlatformEdgeRender[] = []
  /** local fit 结果 */
  @State fitZoomScale: number = 1
  @State fitOffsetX: number = 0
  @State fitOffsetY: number = 0

  // ─── Canvas ───
  private canvasSettings: RenderingContextSettings = new RenderingContextSettings(true)
  private canvasCtx: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.canvasSettings)
  private canvasReady: boolean = false

  // ─── 生命周期 ───

  async aboutToAppear() {
    if (!this.bridge || !this.embed.targetStarmapId) {
      return
    }
    const childScenePath: StarMapPathSegment[] =
      [...this.scenePath, { type: 'enterEmbed', instanceId: this.embed.instanceId, nodeId: null }]
    try {
      const resolveResult = await this.bridge.resolveStarMapPath(this.rootStarmapId, childScenePath)
      if (!resolveResult.success || !resolveResult.data || !resolveResult.data.finalStarmapId) {
        return
      }
      this.finalStarmapId = resolveResult.data.finalStarmapId
      const result = await this.bridge.getStarMapGraph(this.finalStarmapId)
      if (result.success && result.data) {
        this.childGraph = result.data
        this.recomputePreviewGeometry()
      }
    } catch (err) {
      // preview 失败不外显：外壳还是那颗圆壳，只是里面空着。
      hilog.warn(0x0000, 'StarMapEmbedPreview', `load error: ${JSON.stringify(err)}`)
    }
  }

  // ─── 几何 ───

  /**
   * 用和 interactive 层同一套只读几何重建缩略图。
   *
   * local fit 的可用区同样走 computeEmbedInnerContentSafeSide：子图内容不会压到
   * 父圆的标题带与圆环上，缩略图和真实子星图的"留白观感"一致。
   */
  private recomputePreviewGeometry(): void {
    const graph: StarMapGraph | null = this.childGraph
    if (!graph) {
      this.childRects = []
      this.edgeRenders = []
      return
    }
    const nodeLayouts: PlatformLayoutNode[] = buildFreeformLayout(graph.nodes)
    const embedLayouts: PlatformLayoutNode[] = buildEmbedLayoutNodes(graph.embeds)
    const rects: PlatformNodeRect[] =
      nodeLayouts.map((ln: PlatformLayoutNode) => deriveNodeRectFromLayoutNode(ln))
        .concat(embedLayouts.map((ln: PlatformLayoutNode) => deriveNodeRectFromLayoutNode(ln)))
    const rectMap = new Map<string, PlatformNodeRect>()
    for (const rect of rects) {
      rectMap.set(rect.nodeId, rect)
    }
    const embedIds = new Set<string>()
    for (const embed of graph.embeds) {
      embedIds.add(embed.instanceId)
    }
    this.childRects = rects
    this.edgeRenders = computeAllEdgeRenders(graph.edges, rectMap, this.finalStarmapId, embedIds)

    const bounds = computeContentBounds(rects)
    if (!bounds) {
      this.fitZoomScale = 1
      this.fitOffsetX = 0
      this.fitOffsetY = 0
      return
    }
    const available: number = computeEmbedInnerContentSafeSide(PREVIEW_CANVAS_SIZE, PREVIEW_PADDING_VP)
    const fitted = computeFittedViewport(
      bounds, available, available, 0, PREVIEW_CANVAS_SIZE, PREVIEW_CANVAS_SIZE)
    this.fitZoomScale = fitted.zoomScale
    this.fitOffsetX = fitted.offsetX
    this.fitOffsetY = fitted.offsetY
  }

  // ─── 绘制 ───

  /**
   * 画一帧缩略图。
   *
   * 坐标用 Canvas 的 transform 一次性搬进 fit 空间，draw 循环里不再逐点乘比例 ——
   * 这样节点/边/文字三套代码用的是同一份源坐标，不会各乘各的。
   */
  private drawPreview(): void {
    const ctx = this.canvasCtx
    ctx.clearRect(0, 0, ctx.width, ctx.height)
    if (!this.canvasReady || !this.childGraph) {
      return
    }
    const graph: StarMapGraph = this.childGraph
    ctx.save()
    ctx.translate(this.fitOffsetX, this.fitOffsetY)
    ctx.scale(this.fitZoomScale, this.fitZoomScale)

    // 线宽、字号除掉 fit 比例：缩略图是"里面有什么"的示意，
    // 恒定粗细比随缩放一起变粗更可读。
    const unit: number = 1 / Math.max(this.fitZoomScale, 1e-4)

    // 1. 边
    ctx.strokeStyle = this.themeConfig.colors.outline
    ctx.lineWidth = 1.5 * unit
    for (const edge of this.edgeRenders) {
      ctx.beginPath()
      ctx.moveTo(edge.startX, edge.startY)
      ctx.lineTo(edge.endX, edge.endY)
      ctx.stroke()
      ctx.beginPath()
      ctx.moveTo(edge.arrowTipX, edge.arrowTipY)
      ctx.lineTo(edge.arrowLeftX, edge.arrowLeftY)
      ctx.moveTo(edge.arrowTipX, edge.arrowTipY)
      ctx.lineTo(edge.arrowRightX, edge.arrowRightY)
      ctx.stroke()
    }

    // 2. 子节点：圆角矩形，和 interactive 层的形状一致
    ctx.fillStyle = this.themeConfig.colors.surfaceContainerHigh
    ctx.strokeStyle = this.themeConfig.colors.outlineVariant
    for (const rect of this.childRects) {
      if (this.isChildEmbedId(graph, rect.nodeId)) {
        continue
      }
      const r: number = Math.min(DEFAULT_NODE_RADIUS, Math.min(rect.width, rect.height) / 2)
      ctx.beginPath()
      ctx.moveTo(rect.x + r, rect.y)
      ctx.lineTo(rect.x + rect.width - r, rect.y)
      ctx.quadraticCurveTo(rect.x + rect.width, rect.y, rect.x + rect.width, rect.y + r)
      ctx.lineTo(rect.x + rect.width, rect.y + rect.height - r)
      ctx.quadraticCurveTo(rect.x + rect.width, rect.y + rect.height, rect.x + rect.width - r, rect.y + rect.height)
      ctx.lineTo(rect.x + r, rect.y + rect.height)
      ctx.quadraticCurveTo(rect.x, rect.y + rect.height, rect.x, rect.y + rect.height - r)
      ctx.lineTo(rect.x, rect.y + r)
      ctx.quadraticCurveTo(rect.x, rect.y, rect.x + r, rect.y)
      ctx.fill()
      ctx.lineWidth = 1 * unit
      ctx.stroke()
    }

    // 3. 子 Embed：仍然是圆（#821：任何缩放下子星图都是子星图）
    for (const embed of graph.embeds) {
      const rect = this.childRects.find((r: PlatformNodeRect) => r.nodeId === embed.instanceId)
      if (!rect) {
        continue
      }
      ctx.beginPath()
      ctx.arc(rect.x + rect.width / 2, rect.y + rect.height / 2, rect.width / 2, 0, Math.PI * 2)
      ctx.fillStyle = this.themeConfig.colors.primaryContainer
      ctx.fill()
      ctx.strokeStyle = this.themeConfig.colors.outline
      ctx.lineWidth = 1.5 * unit
      ctx.stroke()
    }

    // 4. 文字：投影小到看不清就整层不画（阈值按节点边长 × fit 判定）
    const shortestSide: number = this.shortestChildSide()
    if (shortestSide * this.fitZoomScale >= PREVIEW_LABEL_MIN_SIDE_VP) {
      ctx.fillStyle = this.themeConfig.colors.onSurface
      ctx.font = `${13 * unit}px ${this.themeConfig.fontFamily}`
      ctx.textAlign = 'center'
      ctx.textBaseline = 'middle'
      for (const node of graph.nodes) {
        const rect = this.childRects.find((r: PlatformNodeRect) => r.nodeId === node.id)
        if (!rect) {
          continue
        }
        ctx.fillText(this.nodeLabel(node.title), rect.x + rect.width / 2, rect.y + rect.height / 2)
      }
    }

    ctx.restore()
  }

  private isChildEmbedId(graph: StarMapGraph, nodeId: string): boolean {
    for (const embed of graph.embeds) {
      if (embed.instanceId === nodeId) {
        return true
      }
    }
    return false
  }

  private shortestChildSide(): number {
    let shortest: number = Number.MAX_VALUE
    for (const rect of this.childRects) {
      const side: number = Math.min(rect.width, rect.height)
      if (side < shortest) {
        shortest = side
      }
    }
    return shortest === Number.MAX_VALUE ? 0 : shortest
  }

  private nodeLabel(title: string | null): string {
    const text: string = title ?? ''
    return text.length > 6 ? text.substring(0, 6) : text
  }

  // ─── build ───

  build() {
    Canvas(this.canvasCtx)
      // 固定尺寸：本组件不随缩放改布局（#821）
      .width(PREVIEW_CANVAS_SIZE)
      .height(PREVIEW_CANVAS_SIZE)
      // 整块不参与命中：preview 不抢事件，根 Scene 的两指 Pinch 完整穿过它
      .hitTestBehavior(HitTestMode.None)
      .onReady(() => {
        this.canvasReady = true
        this.drawPreview()
      })
  }
}
