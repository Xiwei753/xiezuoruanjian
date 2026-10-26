// WidePeriodHeatmap — 时段详情内的实际每日码字热力图（Issue #1002）。
// 所有数字和覆盖状态来自 Core 年度热力图，不从趋势桶推算每日数据。

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

interface PeriodHeatmapMonthLabel {
  key: string
  weekIndex: number
  label: string
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
const PERIOD_HEATMAP_LONG_CELL_MAX_VP: number = 18
const PERIOD_HEATMAP_DAY_CELL_MAX_VP: number = 46
const PERIOD_HEATMAP_MONTH_CELL_MAX_VP: number = 42
const PERIOD_HEATMAP_WEEKDAY_LABELS: string[] = ['一', '二', '三', '四', '五', '六', '日']

@Component
export struct WidePeriodHeatmap {
  @Prop @Watch('onRangeChanged') startDate: string = ''
  @Prop @Watch('onRangeChanged') endDate: string = ''
  @Prop days: WritingStatsHeatmapDay[] = []
  @Prop loadedYears: number[] = []
  @Prop isLoading: boolean = false
  @Prop errorMessage: string = ''
  onRetry: () => void = (): void => {}
  onVisibleYears: (years: number[]) => void = (_years: number[]): void => {}

  @State private selectedDate: string = ''
  @State private measuredWidthVp: number = 0
  @State private measuredHeightVp: number = 0
  @State private scrollViewportWidthVp: number = 0

  private scroller: Scroller = new Scroller()
  private lastScrolledRange: string = ''
  private lastScrolledWidthVp: number = -1
  private lastVisibleYearsKey: string = ''

  private rangeKey(): string {
    return `${this.startDate}/${this.endDate}`
  }

  private onRangeChanged(): void {
    this.selectedDate = ''
    this.lastScrolledRange = ''
    this.lastVisibleYearsKey = ''
    this.scheduleInitialLayout()
  }

  private validRange(): boolean {
    const start: Date | null = parseIsoDate(this.startDate)
    const end: Date | null = parseIsoDate(this.endDate)
    return start !== null && end !== null && formatIsoDate(start) === this.startDate &&
      formatIsoDate(end) === this.endDate && start.getTime() <= end.getTime()
  }

  private dayCount(): number {
    if (!this.validRange()) {
      return 0
    }
    const start: Date | null = parseIsoDate(this.startDate)
    const end: Date | null = parseIsoDate(this.endDate)
    if (start === null || end === null) {
      return 0
    }
    const firstUtc: number = Date.UTC(start.getFullYear(), start.getMonth(), start.getDate())
    const lastUtc: number = Date.UTC(end.getFullYear(), end.getMonth(), end.getDate())
    return Math.floor((lastUtc - firstUtc) / 86400000) + 1
  }

  private isShortRange(): boolean {
    return this.dayCount() <= 7
  }

  private isMonthRange(): boolean {
    return this.dayCount() > 7 && this.dayCount() <= 62
  }

  private isLongRange(): boolean {
    return this.dayCount() > 62
  }

  private weekCount(): number {
    const start: Date | null = parseIsoDate(this.startDate)
    if (start === null) {
      return 0
    }
    const leading: number = (start.getDay() + 6) % 7
    return Math.ceil((leading + this.dayCount()) / 7)
  }

  private contentWidthVp(): number {
    return Math.max(0, this.measuredWidthVp * 0.92)
  }

  private shortSlotWidthVp(): number {
    return this.contentWidthVp() / Math.max(1, this.dayCount())
  }

  private monthSlotWidthVp(): number {
    return this.contentWidthVp() / 7
  }

  private longCellStepVp(): number {
    return this.longCellSizeVp() + PERIOD_HEATMAP_CELL_GAP_VP
  }

  private shortCellSizeVp(): number {
    const count: number = Math.max(1, this.dayCount())
    const fitWidth: number = (this.contentWidthVp() - (count - 1) * 5) / count
    const fitHeight: number = Math.max(18, this.measuredHeightVp - 102)
    return Math.max(18, Math.min(PERIOD_HEATMAP_DAY_CELL_MAX_VP, fitWidth, fitHeight))
  }

  private monthCellSizeVp(): number {
    const rows: number = Math.max(1, this.weekCount())
    const fitWidth: number = (this.contentWidthVp() - 6 * PERIOD_HEATMAP_CELL_GAP_VP) / 7
    const availableHeight: number = Math.max(0, this.measuredHeightVp - 116)
    const fitHeight: number = (availableHeight - (rows - 1) * PERIOD_HEATMAP_CELL_GAP_VP) / rows
    return Math.max(11, Math.min(PERIOD_HEATMAP_MONTH_CELL_MAX_VP, fitWidth, fitHeight))
  }

  private longCellSizeVp(): number {
    const visibleWeekTarget: number = 14
    const fitWidth: number = (this.scrollViewportWidthVp -
      (visibleWeekTarget - 1) * PERIOD_HEATMAP_CELL_GAP_VP) / visibleWeekTarget
    const fitHeight: number = (this.measuredHeightVp - 118 - 6 * PERIOD_HEATMAP_CELL_GAP_VP) / 7
    return Math.max(PERIOD_HEATMAP_MIN_CELL_VP,
      Math.min(PERIOD_HEATMAP_LONG_CELL_MAX_VP, fitWidth, fitHeight))
  }

  private longGridWidthVp(): number {
    const weeks: number = this.weekCount()
    return weeks > 0 ? weeks * this.longCellSizeVp() + (weeks - 1) * PERIOD_HEATMAP_CELL_GAP_VP : 0
  }

  private weekdayIndex(date: Date): number {
    return (date.getDay() + 6) % 7
  }

  private weekdayLabel(cell: PeriodHeatmapCell): string {
    const date: Date | null = parseIsoDate(cell.date)
    return date !== null ? PERIOD_HEATMAP_WEEKDAY_LABELS[this.weekdayIndex(date)] : ''
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
      const year: number = Number(this.selectedDate.substring(0, 4))
      return this.loadedYears.includes(year)
        ? `${this.selectedDate} · 数据不完整`
        : `${this.selectedDate} · 每日数据加载中`
    }
    if (day.status === 'unknown') {
      return `${day.date} · 数据不完整`
    }
    if (day.status === 'future') {
      return `${day.date} · 未到`
    }
    return `${day.date} · ${formatInt(day.humanTypedChars)} 字`
  }

  private buildCell(date: Date, inRange: boolean, key: string,
    dayByDate: Map<string, WritingStatsHeatmapDay>, todayKey: string): PeriodHeatmapCell {
    if (!inRange) {
      return { key: key, date: '', chars: 0, intensity: 0, status: '', inRange: false }
    }
    const dateKey: string = formatIsoDate(date)
    const day: WritingStatsHeatmapDay | undefined = dayByDate.get(dateKey)
    const year: number = date.getFullYear()
    const status: string = day !== undefined ? day.status :
      (!this.loadedYears.includes(year) ? 'unloaded' : (dateKey > todayKey ? 'future' : 'unknown'))
    return {
      key: dateKey,
      date: dateKey,
      chars: day !== undefined ? day.humanTypedChars : 0,
      intensity: day !== undefined ? day.intensity : 0,
      status: status,
      inRange: true
    }
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
    const dayByDate: Map<string, WritingStatsHeatmapDay> = new Map<string, WritingStatsHeatmapDay>()
    for (const day of this.days) {
      dayByDate.set(day.date, day)
    }
    const todayKey: string = formatIsoDate(new Date())
    const leading: number = this.weekdayIndex(start)
    let cursor: Date = new Date(start.getFullYear(), start.getMonth(), start.getDate() - leading)
    const weeks: PeriodHeatmapWeek[] = []
    for (let weekIndex: number = 0; weekIndex < this.weekCount(); weekIndex++) {
      const cells: PeriodHeatmapCell[] = []
      for (let offset: number = 0; offset < 7; offset++) {
        const date: Date = new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate() + offset)
        const dateKey: string = formatIsoDate(date)
        const inRange: boolean = dateKey >= this.startDate && dateKey <= this.endDate
        cells.push(this.buildCell(date, inRange, `blank-${weekIndex}-${offset}`, dayByDate, todayKey))
      }
      weeks.push({ key: `week-${weekIndex}`, days: cells })
      cursor = new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate() + 7)
    }
    return weeks
  }

  private buildShortDays(): PeriodHeatmapCell[] {
    const start: Date | null = parseIsoDate(this.startDate)
    if (start === null) {
      return []
    }
    const dayByDate: Map<string, WritingStatsHeatmapDay> = new Map<string, WritingStatsHeatmapDay>()
    for (const day of this.days) {
      dayByDate.set(day.date, day)
    }
    const todayKey: string = formatIsoDate(new Date())
    const result: PeriodHeatmapCell[] = []
    for (let offset: number = 0; offset < this.dayCount(); offset++) {
      const date: Date = new Date(start.getFullYear(), start.getMonth(), start.getDate() + offset)
      result.push(this.buildCell(date, true, formatIsoDate(date), dayByDate, todayKey))
    }
    return result
  }

  private buildMonthLabels(): PeriodHeatmapMonthLabel[] {
    const labels: PeriodHeatmapMonthLabel[] = []
    const weeks: PeriodHeatmapWeek[] = this.buildWeeks()
    const start: Date | null = parseIsoDate(this.startDate)
    if (start === null || weeks.length === 0) {
      return labels
    }
    const seen: string[] = []
    for (let weekIndex: number = 0; weekIndex < weeks.length; weekIndex++) {
      for (const cell of weeks[weekIndex].days) {
        if (!cell.inRange || cell.date.length !== 10) {
          continue
        }
        const monthKey: string = cell.date.substring(0, 7)
        if (seen.includes(monthKey)) {
          continue
        }
        seen.push(monthKey)
        const month: number = Number(cell.date.substring(5, 7))
        labels.push({ key: `month-${monthKey}`, weekIndex: weekIndex, label: `${month}月` })
      }
    }
    return labels
  }

  private cellStyle(cell: PeriodHeatmapCell): PeriodHeatmapCellStyle {
    if (cell.status === 'future') {
      return { color: $r('sys.color.comp_background_gray'), opacity: 0.5, borderWidth: 0 }
    }
    if (cell.status === 'unknown') {
      return { color: Color.Transparent, opacity: 1, borderWidth: 1 }
    }
    if (cell.status === 'unloaded') {
      return { color: $r('sys.color.comp_background_gray'), opacity: 0.28, borderWidth: 0 }
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

  private scheduleInitialLayout(): void {
    setTimeout((): void => {
      this.applyInitialLayout()
    }, 0)
  }

  private applyInitialLayout(): void {
    if (!this.validRange()) {
      return
    }
    const range: string = this.rangeKey()
    if (this.lastScrolledRange === range && Math.abs(this.lastScrolledWidthVp - this.scrollViewportWidthVp) < 0.5) {
      this.reportVisibleYears(0)
      return
    }
    if (this.isLongRange() && this.scrollViewportWidthVp > 0) {
      this.scroller.scrollEdge(Edge.End)
      this.reportVisibleYears(Math.max(0, this.longGridWidthVp() - this.scrollViewportWidthVp))
    } else {
      this.reportVisibleYears(0)
    }
    this.lastScrolledRange = range
    this.lastScrolledWidthVp = this.scrollViewportWidthVp
  }

  private visibleYears(offsetVp: number): number[] {
    const start: Date | null = parseIsoDate(this.startDate)
    const end: Date | null = parseIsoDate(this.endDate)
    if (start === null || end === null) {
      return []
    }
    const years: number[] = []
    if (!this.isLongRange()) {
      for (let year: number = start.getFullYear(); year <= end.getFullYear(); year++) {
        years.push(year)
      }
      return years
    }
    const stride: number = this.longCellSizeVp() + PERIOD_HEATMAP_CELL_GAP_VP
    const firstVisible: number = Math.max(0, Math.floor(Math.max(0, offsetVp) / stride) - 1)
    const lastVisible: number = Math.min(this.weekCount() - 1,
      Math.ceil((Math.max(0, offsetVp) + this.scrollViewportWidthVp) / stride) + 1)
    const leading: number = this.weekdayIndex(start)
    const firstMonday: Date = new Date(start.getFullYear(), start.getMonth(), start.getDate() - leading)
    for (let weekIndex: number = firstVisible; weekIndex <= lastVisible; weekIndex++) {
      for (let dayOffset: number = 0; dayOffset < 7; dayOffset++) {
        const date: Date = new Date(firstMonday.getFullYear(), firstMonday.getMonth(),
          firstMonday.getDate() + weekIndex * 7 + dayOffset)
        const dateKey: string = formatIsoDate(date)
        if (dateKey >= this.startDate && dateKey <= this.endDate && !years.includes(date.getFullYear())) {
          years.push(date.getFullYear())
        }
      }
    }
    years.sort((left: number, right: number) => left - right)
    return years
  }

  private reportVisibleYears(offsetVp: number): void {
    const years: number[] = this.visibleYears(offsetVp)
    const yearsKey: string = years.join(',')
    if (years.length === 0 || yearsKey === this.lastVisibleYearsKey) {
      return
    }
    this.lastVisibleYearsKey = yearsKey
    this.onVisibleYears(years)
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
  private DayTile(cell: PeriodHeatmapCell, size: number, showDate: boolean) {
    Column() {
      if (showDate) {
        Text(cell.date.substring(8, 10))
          .fontSize(size >= 22 ? 9 : 8)
          .fontColor(cell.status === 'known' && cell.intensity > 0
            ? $r('sys.color.font_primary')
            : $r('sys.color.font_tertiary'))
      }
    }
    .width(size)
    .height(size)
    .justifyContent(FlexAlign.Center)
    .borderRadius(PERIOD_HEATMAP_CELL_RADIUS_VP)
    .backgroundColor(this.cellStyle(cell).color)
    .opacity(this.cellStyle(cell).opacity)
    .borderWidth(this.selectedDate === cell.date ? 2 : this.cellStyle(cell).borderWidth)
    .borderColor(this.selectedDate === cell.date
      ? $r('sys.color.ohos_id_color_emphasize')
      : $r('sys.color.comp_divider'))
    .onClick(() => {
      this.selectCell(cell)
    })
  }

  @Builder
  private ShortDaysRow() {
    Row() {
      ForEach(this.buildShortDays(), (cell: PeriodHeatmapCell) => {
        Column({ space: 6 }) {
          Text(this.weekdayLabel(cell))
            .fontSize(10)
            .fontColor($r('sys.color.font_tertiary'))
          this.DayTile(cell, this.shortCellSizeVp(), true)
        }
        .width(this.shortSlotWidthVp())
        .alignItems(HorizontalAlign.Center)
      }, (cell: PeriodHeatmapCell) => cell.key)
    }
    .width('100%')
    .layoutWeight(1)
    .alignItems(VerticalAlign.Center)
    .justifyContent(FlexAlign.Center)
  }

  @Builder
  private MonthWeekRow(week: PeriodHeatmapWeek) {
    Row() {
      ForEach(week.days, (cell: PeriodHeatmapCell) => {
        Column() {
          if (cell.inRange) {
            this.DayTile(cell, this.monthCellSizeVp(), true)
          }
        }
        .width(this.monthSlotWidthVp())
        .height(this.monthCellSizeVp())
        .alignItems(HorizontalAlign.Center)
      }, (cell: PeriodHeatmapCell) => cell.key)
    }
    .width('100%')
    .height(this.monthCellSizeVp())
  }

  @Builder
  private MonthCalendar() {
    Column({ space: PERIOD_HEATMAP_CELL_GAP_VP }) {
      Row() {
        ForEach(PERIOD_HEATMAP_WEEKDAY_LABELS, (label: string, index: number) => {
          Text(label)
            .fontSize(9)
            .fontColor($r('sys.color.font_tertiary'))
            .width(this.monthSlotWidthVp())
            .textAlign(TextAlign.Center)
        }, (_label: string, index: number) => `month-weekday-${index}`)
      }
      .width('100%')

      ForEach(this.buildWeeks(), (week: PeriodHeatmapWeek) => {
        this.MonthWeekRow(week)
      }, (week: PeriodHeatmapWeek) => week.key)
    }
    .width('100%')
    .layoutWeight(1)
    .justifyContent(FlexAlign.Center)
  }

  @Builder
  private WeekColumn(week: PeriodHeatmapWeek, weekIndex: number) {
    Column() {
      ForEach(week.days, (cell: PeriodHeatmapCell, dayIndex: number) => {
        if (cell.inRange) {
          Column() {
            this.DayTile(cell, this.longCellSizeVp(), false)
          }
          .width(this.longCellSizeVp())
          .height(this.longCellSizeVp())
            .margin({ bottom: dayIndex < 6 ? PERIOD_HEATMAP_CELL_GAP_VP : 0 })
        } else {
          Column()
            .width(this.longCellSizeVp())
            .height(this.longCellSizeVp())
            .margin({ bottom: dayIndex < 6 ? PERIOD_HEATMAP_CELL_GAP_VP : 0 })
        }
      }, (cell: PeriodHeatmapCell) => cell.key)
    }
    .width(this.longCellSizeVp())
    .margin({ right: weekIndex < this.weekCount() - 1 ? PERIOD_HEATMAP_CELL_GAP_VP : 0 })
  }

  @Builder
  private MonthLabelRow() {
    Stack({ alignContent: Alignment.TopStart }) {
      ForEach(this.buildMonthLabels(), (month: PeriodHeatmapMonthLabel) => {
        Text(month.label)
          .fontSize(9)
          .fontColor($r('sys.color.font_tertiary'))
          .position({ x: month.weekIndex * this.longCellStepVp(), y: 0 })
      }, (month: PeriodHeatmapMonthLabel) => month.key)
    }
    .width(this.longGridWidthVp())
    .height(14)
    .alignContent(Alignment.TopStart)
  }

  @Builder
  private LongCalendarGrid() {
    Row() {
      Column() {
        ForEach(PERIOD_HEATMAP_WEEKDAY_LABELS, (label: string, index: number) => {
          Text(label)
            .fontSize(9)
            .fontColor($r('sys.color.font_tertiary'))
            .width(PERIOD_HEATMAP_WEEKDAY_GUTTER_VP)
            .height(this.longCellSizeVp())
            .textAlign(TextAlign.Center)
            .margin({ bottom: index < 6 ? PERIOD_HEATMAP_CELL_GAP_VP : 0 })
        }, (_label: string, index: number) => `long-weekday-${index}`)
      }
      .width(PERIOD_HEATMAP_WEEKDAY_GUTTER_VP)
      .margin({ top: 14 })

      Scroll(this.scroller) {
        Column() {
          this.MonthLabelRow()
          Row() {
            ForEach(this.buildWeeks(), (week: PeriodHeatmapWeek, index: number) => {
              this.WeekColumn(week, index)
            }, (week: PeriodHeatmapWeek) => week.key)
          }
          .width(this.longGridWidthVp())
          .alignItems(VerticalAlign.Top)
        }
        .width(this.longGridWidthVp())
        .alignItems(HorizontalAlign.Start)
      }
      .layoutWeight(1)
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Off)
      .align(Alignment.Start)
      .onAppear(() => {
        this.scheduleInitialLayout()
      })
      .onScroll((xOffset: number, _yOffset: number): void => {
        this.reportVisibleYears(xOffset)
      })
      .onAreaChange((_oldArea: Area, newArea: Area): void => {
        const nextWidth: number = Number(newArea.width)
        if (nextWidth > 0 && Math.abs(nextWidth - this.scrollViewportWidthVp) >= 0.5) {
          this.scrollViewportWidthVp = nextWidth
          this.scheduleInitialLayout()
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
      Text('0').fontSize(9).fontColor($r('sys.color.font_tertiary'))
      Column()
        .width(9)
        .height(9)
        .borderRadius(2)
        .backgroundColor($r('sys.color.comp_background_gray'))
        .opacity(0.8)
      Text('少').fontSize(9).fontColor($r('sys.color.font_tertiary'))
      ForEach([1, 2, 3, 4], (level: number) => {
        Column()
          .width(9)
          .height(9)
          .borderRadius(2)
          .backgroundColor($r('sys.color.ohos_id_color_emphasize'))
          .opacity(0.28 + level * 0.18)
      }, (level: number) => `level-${level}`)
      Text('多').fontSize(9).fontColor($r('sys.color.font_tertiary'))
      Column().width(9).height(9).borderRadius(2).border({ width: 1, color: $r('sys.color.comp_divider') })
      Text('未知').fontSize(9).fontColor($r('sys.color.font_tertiary'))
      Column()
        .width(9)
        .height(9)
        .borderRadius(2)
        .backgroundColor($r('sys.color.comp_background_gray'))
        .opacity(0.5)
      Text('未到').fontSize(9).fontColor($r('sys.color.font_tertiary'))
    }
    .width('100%')
    .justifyContent(FlexAlign.Center)
  }

  @Builder
  private FooterStatus() {
    if (this.errorMessage.length > 0) {
      Row({ space: 8 }) {
        Text(this.errorMessage)
          .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
          .fontColor($r('sys.color.ohos_id_color_alert'))
          .layoutWeight(1)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
        Button($r('app.string.retry'))
          .fontSize(12)
          .onClick(() => this.onRetry())
      }
      .width('100%')
    } else if (this.isLoading) {
      Row({ space: 6 }) {
        LoadingProgress().width(12).height(12)
        Text(this.isLongRange() ? '滑动加载其他年份' : '每日数据加载中...')
          .fontSize(10)
          .fontColor($r('sys.color.font_tertiary'))
      }
      .width('100%')
      .justifyContent(FlexAlign.Center)
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
      if (this.isShortRange()) {
        this.ShortDaysRow()
      } else if (this.isMonthRange()) {
        this.MonthCalendar()
      } else if (this.validRange()) {
        this.LongCalendarGrid()
      } else {
        Text('时段日期无效')
          .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
          .fontColor($r('sys.color.ohos_id_color_alert'))
          .layoutWeight(1)
      }
      this.LegendRow()
      this.FooterStatus()
    }
    .width('100%')
    .height('100%')
    .alignItems(HorizontalAlign.Start)
    .padding('3.9%')
    .borderRadius(DesignTokens.radiusLg)
    .border({ width: 1, color: $r('sys.color.comp_divider') })
    .backgroundColor($r('sys.color.background_secondary'))
    .onAreaChange((_oldArea: Area, newArea: Area): void => {
      const nextWidth: number = Number(newArea.width)
      const nextHeight: number = Number(newArea.height)
      if (nextWidth > 0) {
        this.measuredWidthVp = nextWidth
      }
      if (nextHeight > 0) {
        this.measuredHeightVp = nextHeight
      }
      this.scheduleInitialLayout()
    })
  }
}
