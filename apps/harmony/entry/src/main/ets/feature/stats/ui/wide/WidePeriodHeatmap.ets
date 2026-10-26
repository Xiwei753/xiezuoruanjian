// WidePeriodHeatmap — 时段详情内的实际每日码字热力图（Issue #1002）。
// 数据只使用 Core 年度热力图返回的真实 WritingStatsHeatmapDay；未覆盖日保持 unknown，
// 未来日期保持 future，不从 Explorer 的按日/周/月趋势桶推算每日值。

import { WritingStatsHeatmapDay } from '../../../../corebridge/dto/StatsDtos'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'
import { COMPACT_CARD_SUPPORTING_FONT_VP, COMPACT_CARD_TITLE_FONT_VP } from '../../../../ui/theme/LayoutTokens'
import { formatInt, formatIsoDate, parseIsoDate } from '../StatsFormat'

interface PeriodHeatmapCell {
  key: string
  date: string
  chars: number
  intensity: number
  status: string
  inRange: boolean
}

interface PeriodHeatmapWeek {
  key: string
  days: PeriodHeatmapCell[]
}

interface PeriodHeatmapCellStyle {
  color: ResourceColor
  opacity: number
  borderWidth: number
}

const PERIOD_HEATMAP_CELL_GAP_VP: number = 3
const PERIOD_HEATMAP_CELL_RADIUS_VP: number = 3
const PERIOD_HEATMAP_WEEKDAY_GUTTER_VP: number = 20
const PERIOD_HEATMAP_MIN_CELL_VP: number = 12
const PERIOD_HEATMAP_MAX_CELL_VP: number = 24
const PERIOD_HEATMAP_WEEKDAY_LABELS: string[] = ['一', '', '三', '', '五', '', '日']

@Component
export struct WidePeriodHeatmap {
  @Prop @Watch('onRangeChanged') startDate: string = ''
  @Prop @Watch('onRangeChanged') endDate: string = ''
  @Prop days: WritingStatsHeatmapDay[] = []
  @Prop isLoading: boolean = false
  @Prop errorMessage: string = ''
  onRetry: () => void = (): void => {}

  @State private selectedDate: string = ''
  @State private measuredWidthVp: number = 0

  private scroller: Scroller = new Scroller()
  private lastScrolledRange: string = ''
  private lastScrolledWidthVp: number = -1

  private rangeKey(): string {
    return `${this.startDate}/${this.endDate}`
  }

  private onRangeChanged(): void {
    this.selectedDate = ''
    this.lastScrolledRange = ''
    this.scheduleInitialScroll()
  }

  private validRange(): boolean {
    const start: Date | null = parseIsoDate(this.startDate)
    const end: Date | null = parseIsoDate(this.endDate)
    return start !== null && end !== null &&
      formatIsoDate(start) === this.startDate && formatIsoDate(end) === this.endDate &&
      start.getTime() <= end.getTime()
  }

  private hasDays(): boolean {
    return this.days.length > 0
  }

  private dayByDate(date: string): WritingStatsHeatmapDay | null {
    for (const day of this.days) {
      if (day.date === date) {
        return day
      }
    }
    return null
  }

  private selectedDayText(): string {
    if (this.selectedDate.length === 0) {
      return '点按方格查看当天人工输入字数'
    }
    const day: WritingStatsHeatmapDay | null = this.dayByDate(this.selectedDate)
    if (day === null) {
      const todayKey: string = formatIsoDate(new Date())
      return this.selectedDate > todayKey ? `${this.selectedDate} · 未到` : `${this.selectedDate} · 数据不完整`
    }
    if (day.status === 'unknown') {
      return `${day.date} · 数据不完整`
    }
    if (day.status === 'future') {
      return `${day.date} · 未到`
    }
    return `${day.date} · ${formatInt(day.humanTypedChars)} 字`
  }

  private formatLocalDate(date: Date): string {
    return formatIsoDate(date)
  }

  private weekCount(): number {
    if (!this.validRange()) {
      return 0
    }
    const start: Date | null = parseIsoDate(this.startDate)
    const end: Date | null = parseIsoDate(this.endDate)
    if (start === null || end === null) {
      return 0
    }
    const leading: number = (start.getDay() + 6) % 7
    const firstUtc: number = Date.UTC(start.getFullYear(), start.getMonth(), start.getDate())
    const lastUtc: number = Date.UTC(end.getFullYear(), end.getMonth(), end.getDate())
    const dayCount: number = Math.floor((lastUtc - firstUtc) / 86400000) + 1
    return Math.ceil((leading + dayCount) / 7)
  }

  private cellSizeVp(): number {
    const weeks: number = this.weekCount()
    if (weeks <= 0 || this.measuredWidthVp <= 0) {
      return PERIOD_HEATMAP_MIN_CELL_VP
    }
    const gapTotal: number = (weeks - 1) * PERIOD_HEATMAP_CELL_GAP_VP
    const fitSize: number = (this.measuredWidthVp - gapTotal) / weeks
    return Math.max(PERIOD_HEATMAP_MIN_CELL_VP, Math.min(PERIOD_HEATMAP_MAX_CELL_VP, fitSize))
  }

  private gridWidthVp(): number {
    const weeks: number = this.weekCount()
    if (weeks <= 0) {
      return 0
    }
    return weeks * this.cellSizeVp() + (weeks - 1) * PERIOD_HEATMAP_CELL_GAP_VP
  }

  private buildWeeks(): PeriodHeatmapWeek[] {
    if (!this.validRange()) {
      return []
    }
    const start: Date | null = parseIsoDate(this.startDate)
    const end: Date | null = parseIsoDate(this.endDate)
    if (start === null || end === null) {
      return []
    }
    const todayKey: string = formatIsoDate(new Date())
    const dayByDate: Map<string, WritingStatsHeatmapDay> = new Map<string, WritingStatsHeatmapDay>()
    for (const day of this.days) {
      dayByDate.set(day.date, day)
    }
    const leading: number = (start.getDay() + 6) % 7
    let cursor: Date = new Date(start.getFullYear(), start.getMonth(), start.getDate() - leading)
    const last: Date = new Date(end.getFullYear(), end.getMonth(), end.getDate())
    const weeks: PeriodHeatmapWeek[] = []
    let weekIndex: number = 0
    while (cursor.getTime() <= last.getTime()) {
      const cells: PeriodHeatmapCell[] = []
      for (let offset: number = 0; offset < 7; offset++) {
        const date: Date = new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate() + offset)
        const dateKey: string = this.formatLocalDate(date)
        const inRange: boolean = dateKey >= this.startDate && dateKey <= this.endDate
        if (!inRange) {
          cells.push({
            key: `blank-${weekIndex}-${offset}`,
            date: '',
            chars: 0,
            intensity: 0,
            status: '',
            inRange: false
          })
          continue
        }
        const day: WritingStatsHeatmapDay | undefined = dayByDate.get(dateKey)
        const status: string = day !== undefined ? day.status : (dateKey > todayKey ? 'future' : 'unknown')
        cells.push({
          key: dateKey,
          date: dateKey,
          chars: day !== undefined ? day.humanTypedChars : 0,
          intensity: day !== undefined ? day.intensity : 0,
          status: status,
          inRange: true
        })
      }
      weeks.push({ key: `week-${weekIndex}`, days: cells })
      cursor = new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate() + 7)
      weekIndex += 1
    }
    return weeks
  }

  private cellStyle(cell: PeriodHeatmapCell): PeriodHeatmapCellStyle {
    if (cell.status === 'future') {
      return { color: $r('sys.color.comp_background_gray'), opacity: 0.5, borderWidth: 0 }
    }
    if (cell.status === 'unknown') {
      return { color: Color.Transparent, opacity: 1, borderWidth: 1 }
    }
    if (cell.intensity === 0) {
      return { color: $r('sys.color.comp_background_gray'), opacity: 0.8, borderWidth: 0 }
    }
    return {
      color: $r('sys.color.ohos_id_color_emphasize'),
      opacity: 0.28 + cell.intensity * 0.18,
      borderWidth: 0
    }
  }

  private selectCell(cell: PeriodHeatmapCell): void {
    if (!cell.inRange || cell.status === 'future') {
      return
    }
    this.selectedDate = cell.date
  }

  private scheduleInitialScroll(): void {
    setTimeout((): void => {
      this.applyInitialScroll()
    }, 0)
  }

  private applyInitialScroll(): void {
    if (this.measuredWidthVp <= 0 || this.weekCount() === 0) {
      return
    }
    const range: string = this.rangeKey()
    if (this.lastScrolledRange === range && Math.abs(this.lastScrolledWidthVp - this.measuredWidthVp) < 0.5) {
      return
    }
    this.scroller.scrollEdge(Edge.End)
    this.lastScrolledRange = range
    this.lastScrolledWidthVp = this.measuredWidthVp
  }

  @Builder
  private HeaderRow() {
    Row() {
      Text('时段热力图')
        .fontSize(COMPACT_CARD_TITLE_FONT_VP)
        .fontWeight(FontWeight.Medium)
        .fontColor($r('sys.color.font_primary'))
      Blank()
      Text(`${this.startDate} - ${this.endDate}`)
        .fontSize(10)
        .fontColor($r('sys.color.font_tertiary'))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
    }
    .width('100%')
    .margin({ bottom: 4 })
  }

  @Builder
  private WeekColumn(week: PeriodHeatmapWeek, weekIndex: number) {
    Column() {
      ForEach(week.days, (cell: PeriodHeatmapCell, dayIndex: number) => {
        if (cell.inRange) {
          Column() {
            Text(`${Number(cell.date.substring(8, 10))}`)
              .fontSize(8)
              .fontColor(cell.status === 'known' && cell.intensity > 0
                ? $r('sys.color.font_primary')
                : $r('sys.color.font_tertiary'))
          }
          .width(this.cellSizeVp())
          .height(this.cellSizeVp())
          .justifyContent(FlexAlign.Center)
          .borderRadius(PERIOD_HEATMAP_CELL_RADIUS_VP)
          .backgroundColor(this.cellStyle(cell).color)
          .opacity(this.cellStyle(cell).opacity)
          .borderWidth(this.selectedDate === cell.date ? 2 : this.cellStyle(cell).borderWidth)
          .borderColor(this.selectedDate === cell.date
            ? $r('sys.color.ohos_id_color_emphasize')
            : $r('sys.color.comp_divider'))
          .margin({ bottom: dayIndex < 6 ? PERIOD_HEATMAP_CELL_GAP_VP : 0 })
          .onClick(() => {
            this.selectCell(cell)
          })
        } else {
          Column()
            .width(this.cellSizeVp())
            .height(this.cellSizeVp())
            .margin({ bottom: dayIndex < 6 ? PERIOD_HEATMAP_CELL_GAP_VP : 0 })
        }
      }, (cell: PeriodHeatmapCell) => cell.key)
    }
    .width(this.cellSizeVp())
    .margin({ right: weekIndex < this.weekCount() - 1 ? PERIOD_HEATMAP_CELL_GAP_VP : 0 })
  }

  @Builder
  private CalendarGrid() {
    Row() {
      Column() {
        ForEach(PERIOD_HEATMAP_WEEKDAY_LABELS, (label: string, index: number) => {
          Text(label)
            .fontSize(9)
            .fontColor($r('sys.color.font_tertiary'))
            .width(PERIOD_HEATMAP_WEEKDAY_GUTTER_VP)
            .height(this.cellSizeVp())
            .textAlign(TextAlign.Center)
            .margin({ bottom: index < PERIOD_HEATMAP_WEEKDAY_LABELS.length - 1 ? PERIOD_HEATMAP_CELL_GAP_VP : 0 })
        }, (_label: string, index: number) => `weekday-${index}`)
      }
      .width(PERIOD_HEATMAP_WEEKDAY_GUTTER_VP)

      Scroll(this.scroller) {
        Row() {
          ForEach(this.buildWeeks(), (week: PeriodHeatmapWeek, index: number) => {
            this.WeekColumn(week, index)
          }, (week: PeriodHeatmapWeek) => week.key)
        }
        .width(this.gridWidthVp())
        .alignItems(VerticalAlign.Top)
      }
      .layoutWeight(1)
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Off)
      .align(Alignment.Start)
      .onAppear(() => {
        this.scheduleInitialScroll()
      })
      .onAreaChange((_oldArea: Area, newArea: Area): void => {
        const nextWidth: number = Number(newArea.width)
        if (nextWidth > 0 && Math.abs(nextWidth - this.measuredWidthVp) >= 0.5) {
          this.measuredWidthVp = nextWidth
          this.scheduleInitialScroll()
        }
      })
    }
    .width('100%')
    .layoutWeight(1)
    .alignItems(VerticalAlign.Top)
  }

  @Builder
  private LegendRow() {
    Row({ space: 3 }) {
      Text('0')
        .fontSize(9)
        .fontColor($r('sys.color.font_tertiary'))
      Column()
        .width(9)
        .height(9)
        .borderRadius(2)
        .backgroundColor($r('sys.color.comp_background_gray'))
        .opacity(0.8)
      Text('少')
        .fontSize(9)
        .fontColor($r('sys.color.font_tertiary'))
      ForEach([1, 2, 3, 4], (level: number) => {
        Column()
          .width(9)
          .height(9)
          .borderRadius(2)
          .backgroundColor($r('sys.color.ohos_id_color_emphasize'))
          .opacity(0.28 + level * 0.18)
      }, (level: number) => `level-${level}`)
      Text('多')
        .fontSize(9)
        .fontColor($r('sys.color.font_tertiary'))
      Column()
        .width(9)
        .height(9)
        .borderRadius(2)
        .border({ width: 1, color: $r('sys.color.comp_divider') })
      Text('未知')
        .fontSize(9)
        .fontColor($r('sys.color.font_tertiary'))
      Column()
        .width(9)
        .height(9)
        .borderRadius(2)
        .backgroundColor($r('sys.color.comp_background_gray'))
        .opacity(0.5)
      Text('未到')
        .fontSize(9)
        .fontColor($r('sys.color.font_tertiary'))
    }
    .width('100%')
    .justifyContent(FlexAlign.Center)
  }

  @Builder
  private StatusContent() {
    if (this.isLoading) {
      Row({ space: 8 }) {
        LoadingProgress().width(16).height(16)
        Text('每日数据加载中...')
          .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
          .fontColor($r('sys.color.font_tertiary'))
      }
      .width('100%')
      .layoutWeight(1)
      .justifyContent(FlexAlign.Center)
    } else if (this.errorMessage.length > 0) {
      Column({ space: 8 }) {
        Text(this.errorMessage)
          .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
          .fontColor($r('sys.color.ohos_id_color_alert'))
          .maxLines(2)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
        Button($r('app.string.retry'))
          .fontSize(12)
          .onClick(() => {
            this.onRetry()
          })
      }
      .width('100%')
      .layoutWeight(1)
      .justifyContent(FlexAlign.Center)
      .alignItems(HorizontalAlign.Center)
    } else {
      Text('暂无时段日数据')
        .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
        .fontColor($r('sys.color.font_tertiary'))
        .width('100%')
        .layoutWeight(1)
        .textAlign(TextAlign.Center)
      Button($r('app.string.retry'))
        .fontSize(12)
        .onClick(() => {
          this.onRetry()
        })
    }
  }

  build() {
    Column() {
      this.HeaderRow()
      Text(this.selectedDayText())
        .fontSize(10)
        .fontColor(this.selectedDate.length > 0 ? $r('sys.color.font_primary') : $r('sys.color.font_tertiary'))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .margin({ bottom: 6 })
      if (this.hasDays() && this.validRange()) {
        this.CalendarGrid()
        this.LegendRow()
      } else {
        this.StatusContent()
      }
    }
    .width('100%')
    .height('100%')
    .alignItems(HorizontalAlign.Start)
    .padding('3.9%')
    .borderRadius(DesignTokens.radiusLg)
    .border({ width: 1, color: $r('sys.color.comp_divider') })
    .backgroundColor($r('sys.color.background_secondary'))
  }
}
