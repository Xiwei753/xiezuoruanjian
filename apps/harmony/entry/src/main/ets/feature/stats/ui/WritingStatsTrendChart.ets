// WritingStatsTrendChart — 统计页折线图（Issue #945）。
//
// 只把 Core Explorer 快照 `trend.points` 按日期顺序画成折线，附真实日期刻度与「字」单位：
// - 不重算统计、不补点、不猜日期；数据点全来自 Core（day/week/month 三种粒度只影响刻度格式）。
// - 尺寸改变或数据变化才重绘（lastDrawKey 指纹），不在每帧 build 里重画。
// - 没有事件点（或区间内全部为 0）显示「暂无码字记录」，不画截图里的手绘占位折线。
// - 颜色取应用主题（HarmonyThemeAdapter）的 token，不写死黑白；深浅色切换会重绘。

import { ThemeConfig, ThemeObserver, ThemeObserverImpl, HarmonyThemeAdapter } from '../../../ui/theme/HarmonyThemeAdapter'
import { WritingStatsTrend } from '../../../corebridge/dto/StatsDtos'
import { formatAxisDate, formatInt } from './StatsFormat'

// 画布内边距（vp）：顶部留给「最高 N 字」标注，底部留给日期刻度。
const CHART_PAD_LEFT_VP: number = 10
const CHART_PAD_RIGHT_VP: number = 10
const CHART_PAD_TOP_VP: number = 26
const CHART_PAD_BOTTOM_VP: number = 24
// 横轴最多画几个日期刻度（含首尾）。
const CHART_MAX_DATE_TICKS: number = 5
const CHART_FONT_SIZE_VP: number = 11
const CHART_LINE_WIDTH_VP: number = 2
const CHART_POINT_RADIUS_VP: number = 2.5
// 点太多时不画圆点，只画折线（点会糊成一条带）。
const CHART_MAX_POINT_DOTS: number = 40

@Component
export struct WritingStatsTrendChart {
  @Prop @Watch('onChartInputChanged') trend: WritingStatsTrend = { granularity: 'day', points: [] }
  // 画布可视尺寸（vp），onReady / onAreaChange 更新。
  @State areaWidthVp: number = 0
  @State areaHeightVp: number = 0

  private canvasSettings: RenderingContextSettings = new RenderingContextSettings(true)
  private canvasCtx: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.canvasSettings)
  private canvasReady: boolean = false
  private themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  private themeObserver: ThemeObserver | null = null
  // 上次绘制的内容指纹：尺寸、粒度、点列（首尾日期 + 点数 + 总和）与主题色。
  private lastDrawKey: string = ''

  aboutToAppear(): void {
    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
      self.scheduleDraw()
    })
    HarmonyThemeAdapter.getInstance().addObserver(this.themeObserver)
  }

  aboutToDisappear(): void {
    if (this.themeObserver !== null) {
      HarmonyThemeAdapter.getInstance().removeObserver(this.themeObserver)
      this.themeObserver = null
    }
  }

  private onChartInputChanged(): void {
    this.scheduleDraw()
  }

  /** 尺寸或数据变化才重绘；画布未就绪或尺寸为 0 时直接跳过。 */
  private scheduleDraw(): void {
    if (!this.canvasReady) {
      return
    }
    const width: number = this.areaWidthVp
    const height: number = this.areaHeightVp
    if (width <= 0 || height <= 0) {
      return
    }
    const key: string = this.computeDrawKey(width, height)
    if (key === this.lastDrawKey) {
      return
    }
    this.lastDrawKey = key
    this.draw(width, height)
  }

  private computeDrawKey(width: number, height: number): string {
    const points = this.trend.points
    let total: number = 0
    let firstDate: string = ''
    let lastDate: string = ''
    for (let i = 0; i < points.length; i++) {
      total += points[i].humanTypedChars
      if (i === 0) {
        firstDate = points[i].date
      }
      if (i === points.length - 1) {
        lastDate = points[i].date
      }
    }
    return `${width}x${height}|${this.trend.granularity}|${points.length}|${firstDate}|${lastDate}|${total}|${this.themeConfig.colors.primary}`
  }

  private draw(width: number, height: number): void {
    const ctx: CanvasRenderingContext2D = this.canvasCtx
    ctx.clearRect(0, 0, width, height)
    const labelColor: string = this.themeConfig.colors.textSecondary
    const lineColor: string = this.themeConfig.colors.primary
    const gridColor: string = this.themeConfig.colors.divider
    ctx.font = `${CHART_FONT_SIZE_VP}vp ${this.themeConfig.fontFamily}`
    ctx.textBaseline = 'alphabetic'

    const points = this.trend.points
    if (points.length === 0) {
      ctx.fillStyle = labelColor
      ctx.textAlign = 'center'
      ctx.fillText('暂无码字记录', width / 2, height / 2)
      return
    }

    const plotLeft: number = CHART_PAD_LEFT_VP
    const plotRight: number = width - CHART_PAD_RIGHT_VP
    const plotTop: number = CHART_PAD_TOP_VP
    const plotBottom: number = height - CHART_PAD_BOTTOM_VP
    if (plotRight <= plotLeft || plotBottom <= plotTop) {
      return
    }
    const plotWidth: number = plotRight - plotLeft
    const plotHeight: number = plotBottom - plotTop

    let maxValue: number = 0
    for (let i = 0; i < points.length; i++) {
      if (points[i].humanTypedChars > maxValue) {
        maxValue = points[i].humanTypedChars
      }
    }

    // 基线（区间内没有任何输入时折线贴在基线上）。
    ctx.strokeStyle = gridColor
    ctx.lineWidth = 1
    ctx.beginPath()
    ctx.moveTo(plotLeft, plotBottom)
    ctx.lineTo(plotRight, plotBottom)
    ctx.stroke()

    const step: number = points.length > 1 ? plotWidth / (points.length - 1) : 0
    const xAt = (index: number): number => {
      return points.length > 1 ? plotLeft + step * index : plotLeft + plotWidth / 2
    }
    const yAt = (value: number): number => {
      if (maxValue <= 0) {
        return plotBottom
      }
      return plotBottom - (value / maxValue) * plotHeight
    }

    // 折线
    ctx.strokeStyle = lineColor
    ctx.lineWidth = CHART_LINE_WIDTH_VP
    ctx.lineJoin = 'round'
    ctx.lineCap = 'round'
    ctx.beginPath()
    for (let i = 0; i < points.length; i++) {
      const x: number = xAt(i)
      const y: number = yAt(points[i].humanTypedChars)
      if (i === 0) {
        ctx.moveTo(x, y)
      } else {
        ctx.lineTo(x, y)
      }
    }
    ctx.stroke()

    // 数据点（点太多只画折线）
    if (points.length <= CHART_MAX_POINT_DOTS) {
      ctx.fillStyle = lineColor
      for (let i = 0; i < points.length; i++) {
        ctx.beginPath()
        ctx.arc(xAt(i), yAt(points[i].humanTypedChars), CHART_POINT_RADIUS_VP, 0, Math.PI * 2)
        ctx.fill()
      }
    }

    // 顶部标注：最高值与「字」单位；区间内没有输入时不伪造数字。
    ctx.fillStyle = labelColor
    ctx.textAlign = 'left'
    ctx.fillText(maxValue > 0 ? `最高 ${formatInt(maxValue)} 字` : '暂无码字记录', plotLeft, plotTop - 10)

    // 横轴日期刻度：首尾对齐边缘，中间等距取样。
    const tickCount: number = Math.min(CHART_MAX_DATE_TICKS, points.length)
    for (let t = 0; t < tickCount; t++) {
      const index: number = tickCount === 1 ? 0 : Math.round(t * (points.length - 1) / (tickCount - 1))
      const label: string = formatAxisDate(points[index].date, this.trend.granularity)
      if (index === 0 && index !== points.length - 1) {
        ctx.textAlign = 'left'
        ctx.fillText(label, plotLeft, height - 8)
      } else if (index === points.length - 1) {
        ctx.textAlign = 'right'
        ctx.fillText(label, plotRight, height - 8)
      } else {
        ctx.textAlign = 'center'
        ctx.fillText(label, xAt(index), height - 8)
      }
    }
  }

  build() {
    Canvas(this.canvasCtx)
      .width('100%')
      .height('100%')
      .onReady(() => {
        this.canvasReady = true
        this.areaWidthVp = this.canvasCtx.width
        this.areaHeightVp = this.canvasCtx.height
        this.scheduleDraw()
      })
      .onAreaChange((_oldArea: Area, newArea: Area): void => {
        this.areaWidthVp = Number(newArea.width)
        this.areaHeightVp = Number(newArea.height)
        this.scheduleDraw()
      })
  }
}
