// WideCalendarHeatmap — 宽屏统计的独立码字日历区（Issue #972 复核）。
//
// 位于陪伴横条之后，单独占一块卡片，不参与上方四卡 / 展开框的高度分配。
// 这里从零绘制宽屏日历：月份行 + 52/53 个自然周 × 7 天，数据只来自
// Core 的 WritingStatsHeatmap 快照；不再包旧版 WritingStatsCalendarHeatmap。
// 方格随实际可用宽度等分；低于最小可点按尺寸时只让日历网格横向滚动。

import { WritingStatsHeatmap, WritingStatsHeatmapDay } from '../../../../corebridge/dto/StatsDtos'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'
import { COMPACT_CARD_SUPPORTING_FONT_VP } from '../../../../ui/theme/LayoutTokens'
import { formatInt } from '../StatsFormat'

interface WideHeatmapCell {
  key: string
  date: string
  chars: number
  intensity: number
  status: string
  future: boolean
  inYear: boolean
}

interface WideHeatmapWeek {
  key: string
  days: WideHeatmapCell[]
}

interface WideHeatmapMonthLabel {
  key: string
  weekIndex: number
  label: string
}

interface WideHeatmapCellStyle {
  color: ResourceColor
  opacity: number
  borderWidth: number
}

const WIDE_HEATMAP_LABEL_HEIGHT_VP: number = 16
const WIDE_HEATMAP_CELL_GAP_VP: number = 3
const WIDE_HEATMAP_CELL_RADIUS_VP: number = 2
const WIDE_HEATMAP_WEEKDAY_GUTTER_VP: number = 24
const WIDE_HEATMAP_MIN_CELL_SIZE_VP: number = 12
const WIDE_HEATMAP_MIN_YEAR: number = 1970
const WIDE_HEATMAP_MAX_YEAR: number = 2100
const WIDE_HEATMAP_WEEKDAY_LABELS: string[] = ['一', '', '三', '', '五', '', '']

const WIDE_HEATMAP_STATUS_PADDING: string = '2%'
const WIDE_HEATMAP_HORIZONTAL_PADDING: string = '1.6%'
const WIDE_HEATMAP_VERTICAL_PADDING: string = '1.4%'

const EMPTY_WIDE_HEATMAP: WritingStatsHeatmap = {
  year: WIDE_HEATMAP_MAX_YEAR,
  startDate: '',
  endDate: '',
  historyScope: 'localEvents',
  days: []
}

@Component
export struct WideCalendarHeatmap {
  @Prop @Watch('onHeatmapChanged') heatmap: WritingStatsHeatmap | null = null
  @Prop heatmapYear: number = new Date().getFullYear()
  @Prop heatmapError: string = ''
  onChangeYear: (year: number) => void = (_year: number): void => {}
  onSelectDay: (date: string, chars: number) => void = (_date: string, _chars: number): void => {}
  onRetry: () => void = (): void => {}

  @State private selectedDate: string = ''
  @State private measuredWidthVp: number = 0

  private scroller: Scroller = new Scroller()
  private lastScrolledYear: number = -1
  private lastScrolledWidthVp: number = -1

  private snapshot(): WritingStatsHeatmap {
    return this.heatmap !== null ? this.heatmap : EMPTY_WIDE_HEATMAP
  }

  private onHeatmapChanged(): void {
    if (this.selectedDate.length > 0 && this.selectedDay() === null) {
      this.selectedDate = ''
    }
    this.scheduleInitialScroll()
  }

  private hasHeatmap(): boolean {
    return this.heatmap !== null && this.heatmap.days.length > 0
  }

  private selectedDay(): WritingStatsHeatmapDay | null {
    for (const day of this.snapshot().days) {
      if (day.date === this.selectedDate) {
        return day
      }
    }
    return null
  }

  private selectedDayText(): string {
    if (this.selectedDate.length === 0) {
      return '点按方格查看当天人工输入字数'
    }
    const day: WritingStatsHeatmapDay | null = this.selectedDay()
    if (day === null) {
      return this.selectedDate
    }
    if (day.status === 'unknown') {
      return `${day.date} · 数据不完整`
    }
    if (day.status === 'future') {
      return `${day.date} · 未到`
    }
    return `${day.date} · ${formatInt(day.humanTypedChars)} 字`
  }

  private canPreviousYear(): boolean {
    return this.snapshot().year > WIDE_HEATMAP_MIN_YEAR
  }

  private canNextYear(): boolean {
    return this.snapshot().year < new Date().getFullYear() && this.snapshot().year < WIDE_HEATMAP_MAX_YEAR
  }

  private shiftYear(delta: number): void {
    if (delta < 0 && !this.canPreviousYear()) {
      return
    }
    if (delta > 0 && !this.canNextYear()) {
      return
    }
    this.selectedDate = ''
    this.onChangeYear(this.snapshot().year + delta)
  }

  private weekCount(): number {
    const year: number = this.snapshot().year
    const firstDay: Date = new Date(year, 0, 1)
    const leading: number = (firstDay.getDay() + 6) % 7
    const nextYear: Date = new Date(year + 1, 0, 1)
    const daysInYear: number = Math.round((nextYear.getTime() - firstDay.getTime()) / 86400000)
    return Math.ceil((leading + daysInYear) / 7)
  }

  private cellSizeVp(): number {
    const weeks: number = this.weekCount()
    if (weeks === 0 || this.measuredWidthVp <= 0) {
      return WIDE_HEATMAP_MIN_CELL_SIZE_VP
    }
    const gapTotal: number = (weeks - 1) * WIDE_HEATMAP_CELL_GAP_VP
    const fitSize: number = (this.measuredWidthVp - gapTotal) / weeks
    return Math.max(WIDE_HEATMAP_MIN_CELL_SIZE_VP, fitSize)
  }

  private gridWidthVp(): number {
    const weeks: number = this.weekCount()
    if (weeks === 0) {
      return 0
    }
    return weeks * this.cellSizeVp() + (weeks - 1) * WIDE_HEATMAP_CELL_GAP_VP
  }

  private formatLocalDate(date: Date): string {
    const month: string = `${date.getMonth() + 1}`.padStart(2, '0')
    const day: string = `${date.getDate()}`.padStart(2, '0')
    return `${date.getFullYear()}-${month}-${day}`
  }

  private buildWeeks(): WideHeatmapWeek[] {
    const snapshot: WritingStatsHeatmap = this.snapshot()
    const dayByDate: Map<string, WritingStatsHeatmapDay> = new Map<string, WritingStatsHeatmapDay>()
    for (const day of snapshot.days) {
      dayByDate.set(day.date, day)
    }
    const todayKey: string = this.formatLocalDate(new Date())
    const firstDay: Date = new Date(snapshot.year, 0, 1)
    const leading: number = (firstDay.getDay() + 6) % 7
    const lastDay: Date = new Date(snapshot.year, 11, 31)
    const weeks: WideHeatmapWeek[] = []
    let cursor: Date = new Date(snapshot.year, 0, 1 - leading)
    let weekIndex: number = 0
    while (cursor.getTime() <= lastDay.getTime()) {
      const days: WideHeatmapCell[] = []
      for (let offset: number = 0; offset < 7; offset++) {
        const date: Date = new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate() + offset)
        const inYear: boolean = date.getFullYear() === snapshot.year
        if (!inYear) {
          days.push({
            key: `blank-${weekIndex}-${offset}`,
            date: '',
            chars: 0,
            intensity: 0,
            status: '',
            future: false,
            inYear: false
          })
          continue
        }
        const dateKey: string = this.formatLocalDate(date)
        const day: WritingStatsHeatmapDay | undefined = dayByDate.get(dateKey)
        const status: string = day !== undefined
          ? day.status
          : (dateKey > todayKey ? 'future' : 'unknown')
        days.push({
          key: dateKey,
          date: dateKey,
          chars: day !== undefined ? day.humanTypedChars : 0,
          intensity: day !== undefined ? day.intensity : 0,
          status: status,
          future: status === 'future',
          inYear: true
        })
      }
      weeks.push({ key: `week-${weekIndex}`, days: days })
      cursor = new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate() + 7)
      weekIndex += 1
    }
    return weeks
  }

  private buildMonthLabels(): WideHeatmapMonthLabel[] {
    const labels: WideHeatmapMonthLabel[] = []
    const weeks: WideHeatmapWeek[] = this.buildWeeks()
    for (let weekIndex: number = 0; weekIndex < weeks.length; weekIndex++) {
      for (const day of weeks[weekIndex].days) {
        if (day.inYear && day.date.length === 10 && day.date.substring(8) === '01') {
          const month: number = Number(day.date.substring(5, 7))
          labels.push({
            key: `month-${day.date}`,
            weekIndex: weekIndex,
            label: `${month}月`
          })
        }
      }
    }
    return labels
  }

  private cellStyle(cell: WideHeatmapCell): WideHeatmapCellStyle {
    if (cell.future) {
      return { color: $r('sys.color.comp_background_gray'), opacity: 0.55, borderWidth: 0 }
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

  private selectCell(cell: WideHeatmapCell): void {
    if (cell.future) {
      return
    }
    this.selectedDate = cell.date
    this.onSelectDay(cell.date, cell.chars)
  }

  private scheduleInitialScroll(): void {
    setTimeout((): void => {
      this.applyInitialScroll()
    }, 0)
  }

  private applyInitialScroll(): void {
    if (this.measuredWidthVp <= 0) {
      return
    }
    if (this.lastScrolledYear === this.snapshot().year &&
      Math.abs(this.lastScrolledWidthVp - this.measuredWidthVp) < 0.5) {
      return
    }
    const weeks: WideHeatmapWeek[] = this.buildWeeks()
    if (weeks.length === 0) {
      return
    }
    const currentYear: number = new Date().getFullYear()
    if (this.snapshot().year === currentYear) {
      const todayKey: string = this.formatLocalDate(new Date())
      let todayWeek: number = weeks.length - 1
      for (let weekIndex: number = 0; weekIndex < weeks.length; weekIndex++) {
        for (const day of weeks[weekIndex].days) {
          if (day.date === todayKey) {
            todayWeek = weekIndex
          }
        }
      }
      const maxOffset: number = Math.max(0, this.gridWidthVp() - this.measuredWidthVp)
      const targetOffset: number = todayWeek * (this.cellSizeVp() + WIDE_HEATMAP_CELL_GAP_VP) -
        this.measuredWidthVp + (this.cellSizeVp() + WIDE_HEATMAP_CELL_GAP_VP) * 3
      this.scroller.scrollTo({
        xOffset: Math.min(maxOffset, Math.max(0, targetOffset)),
        yOffset: 0
      })
    } else {
      this.scroller.scrollEdge(Edge.End)
    }
    this.lastScrolledYear = this.snapshot().year
    this.lastScrolledWidthVp = this.measuredWidthVp
  }

  @Builder
  private HeaderRow() {
    Row() {
      Text('码字日历')
        .fontSize(15)
        .fontWeight(FontWeight.Medium)
        .fontColor($r('sys.color.font_primary'))
      Blank()
      Text('‹')
        .fontSize(18)
        .fontColor(this.canPreviousYear() ? $r('sys.color.font_secondary') : $r('sys.color.font_tertiary'))
        .padding({ left: 8, right: 8 })
        .opacity(this.canPreviousYear() ? 1 : 0.4)
        .onClick(() => {
          this.shiftYear(-1)
        })
      Text(`${this.snapshot().year}`)
        .fontSize(13)
        .fontColor($r('sys.color.font_secondary'))
      Text('›')
        .fontSize(18)
        .fontColor(this.canNextYear() ? $r('sys.color.font_secondary') : $r('sys.color.font_tertiary'))
        .padding({ left: 8, right: 8 })
        .opacity(this.canNextYear() ? 1 : 0.4)
        .onClick(() => {
          this.shiftYear(1)
        })
    }
    .width('100%')
    .margin({ bottom: 6 })
  }

  @Builder
  private MonthLabelRow() {
    Stack({ alignContent: Alignment.TopStart }) {
      ForEach(this.buildMonthLabels(), (month: WideHeatmapMonthLabel) => {
        Text(month.label)
          .fontSize(10)
          .fontColor($r('sys.color.font_tertiary'))
          .height(WIDE_HEATMAP_LABEL_HEIGHT_VP)
          .position({
            x: month.weekIndex * (this.cellSizeVp() + WIDE_HEATMAP_CELL_GAP_VP),
            y: 0
          })
      }, (month: WideHeatmapMonthLabel) => month.key)
    }
    .width(this.gridWidthVp())
    .height(WIDE_HEATMAP_LABEL_HEIGHT_VP)
    .alignContent(Alignment.TopStart)
  }

  @Builder
  private WeekColumn(week: WideHeatmapWeek, weekIndex: number) {
    Column() {
      ForEach(week.days, (cell: WideHeatmapCell, dayIndex: number) => {
        if (cell.inYear) {
          Column()
            .width(this.cellSizeVp())
            .height(this.cellSizeVp())
            .borderRadius(WIDE_HEATMAP_CELL_RADIUS_VP)
            .backgroundColor(this.cellStyle(cell).color)
            .opacity(this.cellStyle(cell).opacity)
            .borderWidth(this.cellStyle(cell).borderWidth)
            .borderColor($r('sys.color.comp_divider'))
            .margin({ bottom: dayIndex < 6 ? WIDE_HEATMAP_CELL_GAP_VP : 0 })
            .onClick(() => {
              this.selectCell(cell)
            })
        } else {
          Column()
            .width(this.cellSizeVp())
            .height(this.cellSizeVp())
            .margin({ bottom: dayIndex < 6 ? WIDE_HEATMAP_CELL_GAP_VP : 0 })
        }
      }, (cell: WideHeatmapCell) => cell.key)
    }
    .width(this.cellSizeVp())
    .margin({ right: weekIndex < this.weekCount() - 1 ? WIDE_HEATMAP_CELL_GAP_VP : 0 })
  }

  @Builder
  private CalendarGrid() {
    Row() {
      Column() {
        ForEach(WIDE_HEATMAP_WEEKDAY_LABELS, (label: string, index: number) => {
          Text(label)
            .fontSize(10)
            .fontColor($r('sys.color.font_tertiary'))
            .width(WIDE_HEATMAP_WEEKDAY_GUTTER_VP)
            .height(this.cellSizeVp())
            .textAlign(TextAlign.Center)
            .margin({ bottom: index < WIDE_HEATMAP_WEEKDAY_LABELS.length - 1 ? WIDE_HEATMAP_CELL_GAP_VP : 0 })
        }, (label: string, index: number) => `weekday-${index}`)
      }
      .width(WIDE_HEATMAP_WEEKDAY_GUTTER_VP)
      .margin({ top: WIDE_HEATMAP_LABEL_HEIGHT_VP })

      Scroll(this.scroller) {
        Column() {
          this.MonthLabelRow()
          Row() {
            ForEach(this.buildWeeks(), (week: WideHeatmapWeek, index: number) => {
              this.WeekColumn(week, index)
            }, (week: WideHeatmapWeek) => week.key)
          }
          .width(this.gridWidthVp())
          .alignItems(VerticalAlign.Top)
        }
        .width(this.gridWidthVp())
        .alignItems(HorizontalAlign.Start)
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
    .alignItems(VerticalAlign.Top)
  }

  @Builder
  private LegendRow() {
    Row() {
      Text('少')
        .fontSize(10)
        .fontColor($r('sys.color.font_tertiary'))
      ForEach([0, 1, 2, 3, 4], (level: number) => {
        Column()
          .width(10)
          .height(10)
          .borderRadius(WIDE_HEATMAP_CELL_RADIUS_VP)
          .backgroundColor(level === 0
            ? $r('sys.color.comp_background_gray')
            : $r('sys.color.ohos_id_color_emphasize'))
          .opacity(level === 0 ? 0.8 : 0.28 + level * 0.18)
          .margin({ left: 4, right: 4 })
      }, (level: number) => `legend-${level}`)
      Text('多')
        .fontSize(10)
        .fontColor($r('sys.color.font_tertiary'))
        .margin({ left: 4 })
    }
    .width('100%')
    .justifyContent(FlexAlign.End)
    .margin({ top: 8 })
  }

  @Builder
  private EmptyPlaceholder() {
    Row({ space: 12 }) {
      Text(`${this.heatmapYear} 年暂无码字日历数据`)
        .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
        .fontColor($r('sys.color.font_tertiary'))
        .layoutWeight(1)
      Button($r('app.string.retry'))
        .fontSize(12)
        .onClick(() => {
          this.onRetry()
        })
    }
    .width('100%')
    .padding({ top: WIDE_HEATMAP_STATUS_PADDING, bottom: WIDE_HEATMAP_STATUS_PADDING })
    .alignItems(VerticalAlign.Center)
  }

  @Builder
  private LoadingPlaceholder() {
    Row({ space: 8 }) {
      LoadingProgress()
        .width(16)
        .height(16)
      Text(`${this.heatmapYear} 年 · 码字日历加载中...`)
        .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
        .fontColor($r('sys.color.font_tertiary'))
    }
    .width('100%')
    .padding({ top: WIDE_HEATMAP_STATUS_PADDING, bottom: WIDE_HEATMAP_STATUS_PADDING })
    .justifyContent(FlexAlign.Center)
  }

  @Builder
  private ErrorPlaceholder() {
    Row({ space: 12 }) {
      Text(this.heatmapError)
        .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
        .fontColor($r('sys.color.ohos_id_color_alert'))
        .layoutWeight(1)
        .maxLines(2)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
      Button($r('app.string.retry'))
        .fontSize(12)
        .onClick(() => {
          this.onRetry()
        })
    }
    .width('100%')
    .padding({ top: WIDE_HEATMAP_STATUS_PADDING, bottom: WIDE_HEATMAP_STATUS_PADDING })
    .alignItems(VerticalAlign.Center)
  }

  build() {
    Column() {
      if (this.hasHeatmap()) {
        this.HeaderRow()
        Text(this.selectedDayText())
          .fontSize(12)
          .fontColor(this.selectedDate.length > 0
            ? $r('sys.color.font_primary')
            : $r('sys.color.font_tertiary'))
          .margin({ bottom: 8 })
        this.CalendarGrid()
        this.LegendRow()
      } else if (this.heatmap !== null) {
        this.EmptyPlaceholder()
      } else if (this.heatmapError.length > 0) {
        this.ErrorPlaceholder()
      } else {
        this.LoadingPlaceholder()
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({
      left: WIDE_HEATMAP_HORIZONTAL_PADDING,
      right: WIDE_HEATMAP_HORIZONTAL_PADDING,
      top: WIDE_HEATMAP_VERTICAL_PADDING,
      bottom: WIDE_HEATMAP_VERTICAL_PADDING
    })
    .borderRadius(DesignTokens.cardRadius)
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
  }
}
