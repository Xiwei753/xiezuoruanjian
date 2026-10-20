/*
 * 码字日历热力图（#956 / #968）。
 *
 * 用 Core 的逐日人工输入真值快照静态绘制一年：
 * - 横向按连续自然周排列，纵向固定七行（周一 → 周日）；
 * - 月份标签是网格上方独立的一行，按自然周列定位，文字可跨列，不被单格宽度裁切；
 * - 每格是一个真实公历日：未来日期更浅且不可点击；覆盖未知（数据不完整）
 *   的日期画空心格，与已确认的真实「0 字」区分；
 * - 颜色强度 0–4 由 Core 统一分级，这里只做主题色映射（不硬编码绿色）；
 * - 点按某日显示本地日期与当天人工输入字数；不覆盖未知的日期显示为
 *   数据不完整，不依赖悬停，手机触屏可用。
 *
 * 宽度由实际内容区测量：52/53 个自然周能放下时，方格等分填满可用宽度；
 * 放不下时保持最小可点按尺寸，并由网格内部横向滚动。只做静态绘制，
 * 不用定时器逐帧刷新，外部输入更新不会导致整页重绘。
 */

import { WritingStatsHeatmap, WritingStatsHeatmapDay } from '../../../corebridge/dto/StatsDtos';
import { formatInt } from './StatsFormat';

/** 网格内单格渲染数据。 */
interface HeatmapCell {
  key: string;
  date: string;
  chars: number;
  intensity: number;
  /** Core 覆盖状态：known / unknown / future；空位为空串。 */
  status: string;
  future: boolean;
  inYear: boolean;
}

/** 一列自然周（周一 → 周日，含补齐空位）。 */
interface HeatmapWeek {
  key: string;
  days: HeatmapCell[];
}

/** 网格上方独立月份标签：放到第 weekIndex 个自然周列的开头。 */
interface HeatmapMonthLabel {
  key: string;
  weekIndex: number;
  label: string;
}

interface HeatmapCellStyle {
  color: ResourceColor;
  opacity: number;
  borderWidth: number;
}

const HEATMAP_LABEL_HEIGHT_VP = 16;
const HEATMAP_CELL_GAP_VP = 3;
const HEATMAP_CELL_RADIUS_VP = 2;
const HEATMAP_WEEKDAY_GUTTER_VP = 24;
// 仍可稳定点按的最小方格；再窄就改为网格内部横向滚动。
const HEATMAP_MIN_CELL_SIZE_VP = 12;
const HEATMAP_MIN_YEAR = 1970;
const HEATMAP_MAX_YEAR = 2100;
const WEEKDAY_LABELS: string[] = ['一', '', '三', '', '五', '', ''];

const EMPTY_HEATMAP: WritingStatsHeatmap = {
  year: HEATMAP_MAX_YEAR,
  startDate: '',
  endDate: '',
  historyScope: 'localEvents',
  days: [],
};

@Component
export struct WritingStatsCalendarHeatmap {
  /** Core 每日真值快照（同一年 365/366 项，日期升序）。 */
  @Prop @Watch('onHeatmapChanged') heatmap: WritingStatsHeatmap = EMPTY_HEATMAP;
  /** 年份切换回调：由统计页统一重新查询该年快照。 */
  onChangeYear: (year: number) => void = (year: number) => {};
  /** 日格选择回调：用于统计页需要感知选中日期的场景。 */
  onSelectDay: (date: string, chars: number) => void = (date: string, chars: number) => {};
  @State private selectedDate: string = '';
  // Scroll 实际拿到的横向视口宽度（vp），由 onAreaChange 写入。
  @State private measuredWidthVp: number = 0;
  private scroller: Scroller = new Scroller();
  private lastScrolledYear: number = -1;
  private lastScrolledWidthVp: number = -1;

  build() {
    Column() {
      this.HeaderRow()
      this.SelectedDayRow()
      this.GridSection()
      this.LegendRow()
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
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
        .onClick(() => this.shiftYear(-1))
      Text(`${this.heatmap.year}`)
        .fontSize(13)
        .fontColor($r('sys.color.font_secondary'))
      Text('›')
        .fontSize(18)
        .fontColor(this.canNextYear() ? $r('sys.color.font_secondary') : $r('sys.color.font_tertiary'))
        .padding({ left: 8, right: 8 })
        .opacity(this.canNextYear() ? 1 : 0.4)
        .onClick(() => this.shiftYear(1))
    }
    .width('100%')
    .margin({ bottom: 6 })
  }

  @Builder
  private SelectedDayRow() {
    Text(this.selectedDayText())
      .fontSize(12)
      .fontColor(this.selectedDate !== '' ? $r('sys.color.font_primary') : $r('sys.color.font_tertiary'))
      .margin({ bottom: 8 })
  }

  /** 选中日期的实时字数：每次渲染从当前快照查找，外部同步刷新后立即反映。 */
  private selectedDayText(): string {
    if (this.selectedDate === '') {
      return '点按方格查看当天人工输入字数';
    }
    const day = this.selectedDay();
    if (day === null) {
      return this.selectedDate;
    }
    if (day.status === 'unknown') {
      // 覆盖不完整：不能把缺失当没写作。
      return `${day.date} · 数据不完整`;
    }
    if (day.status === 'future') {
      return `${day.date} · 未到`;
    }
    return `${day.date} · ${formatInt(day.humanTypedChars)} 字`;
  }

  /** 当前快照中选中日期对应的记录；不在该年快照中时返回 null。 */
  private selectedDay(): WritingStatsHeatmapDay | null {
    for (const day of this.heatmap.days) {
      if (day.date === this.selectedDate) {
        return day;
      }
    }
    return null;
  }

  /** 快照换年或刷新后，选中日期已不在当前年份时清掉选中，并重新定位横向滚动。 */
  private onHeatmapChanged(): void {
    if (this.selectedDate !== '' && this.selectedDay() === null) {
      this.selectedDate = '';
    }
    this.scheduleInitialScroll();
  }

  @Builder
  private GridSection() {
    Row() {
      // 星期标签固定在最左，不参与横向滚动；顶部让出月份标签行。
      Column() {
        ForEach(WEEKDAY_LABELS, (label: string, index: number) => {
          Text(label)
            .fontSize(10)
            .fontColor($r('sys.color.font_tertiary'))
            .width(HEATMAP_WEEKDAY_GUTTER_VP)
            .height(this.cellSizeVp())
            .textAlign(TextAlign.Center)
            .margin({ bottom: index < WEEKDAY_LABELS.length - 1 ? HEATMAP_CELL_GAP_VP : 0 })
        }, (label: string, index: number) => `weekday-${index}`)
      }
      .width(HEATMAP_WEEKDAY_GUTTER_VP)
      .margin({ top: HEATMAP_LABEL_HEIGHT_VP })

      Scroll(this.scroller) {
        Column() {
          this.MonthLabelRow()
          Row() {
            ForEach(this.buildWeeks(), (week: HeatmapWeek, index: number) => {
              this.WeekColumn(week, index)
            }, (week: HeatmapWeek) => week.key)
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
        this.scheduleInitialScroll();
      })
      .onAreaChange((_oldArea: Area, newArea: Area): void => {
        const nextWidth: number = Number(newArea.width);
        if (nextWidth > 0 && Math.abs(nextWidth - this.measuredWidthVp) >= 0.5) {
          this.measuredWidthVp = nextWidth;
          this.scheduleInitialScroll();
        }
      })
    }
    .width('100%')
    .alignItems(VerticalAlign.Top)
  }

  /** 月份标签独立一行，按自然周列定位；Text 自身宽度可跨多个格子。 */
  @Builder
  private MonthLabelRow() {
    Stack({ alignContent: Alignment.TopStart }) {
      ForEach(this.buildMonthLabels(), (month: HeatmapMonthLabel) => {
        Text(month.label)
          .fontSize(10)
          .fontColor($r('sys.color.font_tertiary'))
          .height(HEATMAP_LABEL_HEIGHT_VP)
          .position({
            x: month.weekIndex * (this.cellSizeVp() + HEATMAP_CELL_GAP_VP),
            y: 0
          })
      }, (month: HeatmapMonthLabel) => month.key)
    }
    .width(this.gridWidthVp())
    .height(HEATMAP_LABEL_HEIGHT_VP)
    .alignContent(Alignment.TopStart)
  }

  @Builder
  private WeekColumn(week: HeatmapWeek, weekIndex: number) {
    Column() {
      ForEach(week.days, (cell: HeatmapCell, dayIndex: number) => {
        if (cell.inYear) {
          Column()
            .width(this.cellSizeVp())
            .height(this.cellSizeVp())
            .borderRadius(HEATMAP_CELL_RADIUS_VP)
            .backgroundColor(this.cellStyle(cell).color)
            .opacity(this.cellStyle(cell).opacity)
            .borderWidth(this.cellStyle(cell).borderWidth)
            .borderColor($r('sys.color.comp_divider'))
            .margin({ bottom: dayIndex < 6 ? HEATMAP_CELL_GAP_VP : 0 })
            .onClick(() => this.selectCell(cell))
        } else {
          Column()
            .width(this.cellSizeVp())
            .height(this.cellSizeVp())
            .margin({ bottom: dayIndex < 6 ? HEATMAP_CELL_GAP_VP : 0 })
        }
      }, (cell: HeatmapCell) => cell.key)
    }
    .width(this.cellSizeVp())
    .margin({ right: weekIndex < this.weekCount() - 1 ? HEATMAP_CELL_GAP_VP : 0 })
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
          .borderRadius(HEATMAP_CELL_RADIUS_VP)
          .backgroundColor(level === 0 ? $r('sys.color.comp_background_gray') : $r('sys.color.ohos_id_color_emphasize'))
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

  private canPreviousYear(): boolean {
    return this.heatmap.year > HEATMAP_MIN_YEAR;
  }

  private canNextYear(): boolean {
    return this.heatmap.year < new Date().getFullYear() && this.heatmap.year < HEATMAP_MAX_YEAR;
  }

  private shiftYear(delta: number): void {
    if (delta < 0 && !this.canPreviousYear()) {
      return;
    }
    if (delta > 0 && !this.canNextYear()) {
      return;
    }
    this.selectedDate = '';
    this.onChangeYear(this.heatmap.year + delta);
  }

  private selectCell(cell: HeatmapCell): void {
    if (cell.future) {
      // 未来日期不伪造成未写作，也不可点击。
      return;
    }
    this.selectedDate = cell.date;
    this.onSelectDay(cell.date, cell.chars);
  }

  private cellStyle(cell: HeatmapCell): HeatmapCellStyle {
    if (cell.future) {
      // 浅色主题下 0.35 几乎看不见；未来日仍要能看出这是日历的一部分。
      return { color: $r('sys.color.comp_background_gray'), opacity: 0.55, borderWidth: 0 };
    }
    if (cell.status === 'unknown') {
      // 覆盖未知：空心/中性色，区别于已确认的真实 0。
      return { color: Color.Transparent, opacity: 1, borderWidth: 1 };
    }
    if (cell.intensity === 0) {
      return { color: $r('sys.color.comp_background_gray'), opacity: 0.8, borderWidth: 0 };
    }
    return {
      color: $r('sys.color.ohos_id_color_emphasize'),
      opacity: 0.28 + cell.intensity * 0.18,
      borderWidth: 0,
    };
  }

  /** 当前年份的自然周数（52/53），O(1) 计算，供方格尺寸与列间距使用。 */
  private weekCount(): number {
    const firstDay: Date = new Date(this.heatmap.year, 0, 1);
    const leading: number = (firstDay.getDay() + 6) % 7;
    const nextYear: Date = new Date(this.heatmap.year + 1, 0, 1);
    const daysInYear: number = Math.round((nextYear.getTime() - firstDay.getTime()) / 86400000);
    return Math.ceil((leading + daysInYear) / 7);
  }

  /** 当前可用宽度下单个方格的边长：能放下就等分，放不下就保底并横向滚动。 */
  private cellSizeVp(): number {
    const weekCount: number = this.weekCount();
    if (weekCount === 0 || this.measuredWidthVp <= 0) {
      return HEATMAP_MIN_CELL_SIZE_VP;
    }
    const gapTotal: number = (weekCount - 1) * HEATMAP_CELL_GAP_VP;
    const fitSize: number = (this.measuredWidthVp - gapTotal) / weekCount;
    return Math.max(HEATMAP_MIN_CELL_SIZE_VP, fitSize);
  }

  /** 所有自然周列 + 列间距的总宽度；放得下时正好等于滚动视口。 */
  private gridWidthVp(): number {
    const weekCount: number = this.weekCount();
    if (weekCount === 0) {
      return 0;
    }
    return weekCount * this.cellSizeVp() + (weekCount - 1) * HEATMAP_CELL_GAP_VP;
  }

  /** 初始定位：当前年份优先露出包含今天的周；历史年份滚到年末。 */
  private scheduleInitialScroll(): void {
    setTimeout((): void => {
      this.applyInitialScroll();
    }, 0);
  }

  private applyInitialScroll(): void {
    if (this.measuredWidthVp <= 0) {
      return;
    }
    if (this.lastScrolledYear === this.heatmap.year &&
      Math.abs(this.lastScrolledWidthVp - this.measuredWidthVp) < 0.5) {
      return;
    }
    const weeks: HeatmapWeek[] = this.buildWeeks();
    if (weeks.length === 0) {
      return;
    }
    const currentYear: number = new Date().getFullYear();
    if (this.heatmap.year === currentYear) {
      const todayKey: string = this.formatLocalDate(new Date());
      let todayWeek: number = weeks.length - 1;
      for (let weekIndex = 0; weekIndex < weeks.length; weekIndex++) {
        for (const day of weeks[weekIndex].days) {
          if (day.date === todayKey) {
            todayWeek = weekIndex;
          }
        }
      }
      const maxOffset: number = Math.max(0, this.gridWidthVp() - this.measuredWidthVp);
      const targetOffset: number = todayWeek * (this.cellSizeVp() + HEATMAP_CELL_GAP_VP)
        - this.measuredWidthVp
        + (this.cellSizeVp() + HEATMAP_CELL_GAP_VP) * 3;
      this.scroller.scrollTo({
        xOffset: Math.min(maxOffset, Math.max(0, targetOffset)),
        yOffset: 0
      });
    } else {
      this.scroller.scrollEdge(Edge.End);
    }
    this.lastScrolledYear = this.heatmap.year;
    this.lastScrolledWidthVp = this.measuredWidthVp;
  }

  /** 按本地日历把一年切成自然周；前置空位补到第一个周一。 */
  private buildWeeks(): HeatmapWeek[] {
    const snapshot = this.heatmap;
    const dayByDate = new Map<string, WritingStatsHeatmapDay>();
    for (const day of snapshot.days) {
      dayByDate.set(day.date, day);
    }
    const todayKey = this.formatLocalDate(new Date());
    const firstDay = new Date(snapshot.year, 0, 1);
    const leading = (firstDay.getDay() + 6) % 7;
    const lastDay = new Date(snapshot.year, 11, 31);
    const weeks: HeatmapWeek[] = [];
    let cursor = new Date(snapshot.year, 0, 1 - leading);
    let weekIndex = 0;
    while (cursor.getTime() <= lastDay.getTime()) {
      const days: HeatmapCell[] = [];
      for (let offset = 0; offset < 7; offset++) {
        const date = new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate() + offset);
        const inYear = date.getFullYear() === snapshot.year;
        if (!inYear) {
          days.push({
            key: `blank-${weekIndex}-${offset}`,
            date: '',
            chars: 0,
            intensity: 0,
            status: '',
            future: false,
            inYear: false,
          });
          continue;
        }
        const dateKey = this.formatLocalDate(date);
        const day = dayByDate.get(dateKey);
        const status = day !== undefined
          ? day.status
          : (dateKey > todayKey ? 'future' : 'unknown');
        days.push({
          key: dateKey,
          date: dateKey,
          chars: day !== undefined ? day.humanTypedChars : 0,
          intensity: day !== undefined ? day.intensity : 0,
          status,
          future: status === 'future',
          inYear: true,
        });
      }
      weeks.push({ key: `week-${weekIndex}`, days });
      cursor = new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate() + 7);
      weekIndex += 1;
    }
    return weeks;
  }

  /** 从自然周网格里取出每月一日所在列，生成独立月份标签行。 */
  private buildMonthLabels(): HeatmapMonthLabel[] {
    const labels: HeatmapMonthLabel[] = [];
    const weeks: HeatmapWeek[] = this.buildWeeks();
    for (let weekIndex = 0; weekIndex < weeks.length; weekIndex++) {
      for (const day of weeks[weekIndex].days) {
        if (day.inYear && day.date.length === 10 && day.date.substring(8) === '01') {
          const month: number = Number(day.date.substring(5, 7));
          labels.push({
            key: `month-${day.date}`,
            weekIndex: weekIndex,
            label: `${month}月`
          });
        }
      }
    }
    return labels;
  }

  /** 按本地日期生成 YYYY-MM-DD，避免 UTC 解析换日。 */
  private formatLocalDate(date: Date): string {
    const month = `${date.getMonth() + 1}`.padStart(2, '0');
    const day = `${date.getDate()}`.padStart(2, '0');
    return `${date.getFullYear()}-${month}-${day}`;
  }
}
