/*
 * 码字日历热力图（#956）。
 *
 * 用 Core 的逐日人工输入真值快照静态绘制一年：
 * - 横向按连续自然周排列，纵向固定七行（周一 → 周日）；
 * - 每格是一个真实公历日：未来日期更浅且不可点击；覆盖未知（数据不完整）
 *   的日期画空心格，与已确认的真实「0 字」区分；
 * - 颜色强度 0–4 由 Core 统一分级，这里只做主题色映射（不硬编码绿色）；
 * - 点按某日显示本地日期与当天人工输入字数；不覆盖未知的日期显示为
 *   数据不完整，不依赖悬停，手机触屏可用。
 *
 * 只做静态绘制：不用定时器、不逐帧刷新；宽度变化由声明式布局自然重排，
 * 外部输入更新不会导致整页重绘。
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
  monthLabel: string;
  days: HeatmapCell[];
}

interface HeatmapCellStyle {
  color: ResourceColor;
  opacity: number;
  borderWidth: number;
}

/** 横向滚动初始定位到最新周的滚动器。 */
const HEATMAP_LABEL_HEIGHT_VP = 14;
const HEATMAP_CELL_GAP_VP = 3;
const HEATMAP_CELL_RADIUS_VP = 2;
const HEATMAP_WEEKDAY_GUTTER_VP = 24;
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
  /** 窄屏约 10–13vp；宽屏可略大。 */
  @Prop cellSizeVp: number = 12;
  /** 年份切换回调：由统计页统一重新查询该年快照。 */
  onChangeYear: (year: number) => void = (year: number) => {};
  /** 日格选择回调：用于统计页需要感知选中日期的场景。 */
  onSelectDay: (date: string, chars: number) => void = (date: string, chars: number) => {};
  @State private selectedDate: string = '';
  private scroller: Scroller = new Scroller();

  build() {
    Column() {
      this.HeaderRow()
      this.SelectedDayRow()
      this.GridRow()
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

  /** 快照换年或刷新后，选中日期已不在当前年份时清掉选中。 */
  private onHeatmapChanged(): void {
    if (this.selectedDate !== '' && this.selectedDay() === null) {
      this.selectedDate = '';
    }
  }

  @Builder
  private GridRow() {
    Row() {
      Column() {
        ForEach(WEEKDAY_LABELS, (label: string, index: number) => {
          Text(label)
            .fontSize(10)
            .fontColor($r('sys.color.font_tertiary'))
            .width(HEATMAP_WEEKDAY_GUTTER_VP)
            .height(this.cellSizeVp)
            .textAlign(TextAlign.Center)
            .margin({ bottom: HEATMAP_CELL_GAP_VP })
        }, (label: string, index: number) => `weekday-${index}`)
      }
      .margin({ top: HEATMAP_LABEL_HEIGHT_VP })

      Scroll(this.scroller) {
        Row() {
          ForEach(this.buildWeeks(), (week: HeatmapWeek) => {
            this.WeekColumn(week)
          }, (week: HeatmapWeek) => week.key)
        }
      }
      .layoutWeight(1)
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Off)
      .align(Alignment.Start)
      .onAppear(() => {
        // 初始定位最近日期：当前年份滚到最右端。
        this.scroller.scrollEdge(Edge.End);
      })
    }
    .width('100%')
    .alignItems(VerticalAlign.Top)
  }

  @Builder
  private WeekColumn(week: HeatmapWeek) {
    Column() {
      Text(week.monthLabel)
        .fontSize(10)
        .fontColor($r('sys.color.font_tertiary'))
        .height(HEATMAP_LABEL_HEIGHT_VP)
        .width('100%')
      ForEach(week.days, (cell: HeatmapCell) => {
        if (cell.inYear) {
          Column()
            .width(this.cellSizeVp)
            .height(this.cellSizeVp)
            .borderRadius(HEATMAP_CELL_RADIUS_VP)
            .backgroundColor(this.cellStyle(cell).color)
            .opacity(this.cellStyle(cell).opacity)
            .borderWidth(this.cellStyle(cell).borderWidth)
            .borderColor($r('sys.color.comp_divider'))
            .margin({ bottom: HEATMAP_CELL_GAP_VP })
            .onClick(() => this.selectCell(cell))
        } else {
          Column()
            .width(this.cellSizeVp)
            .height(this.cellSizeVp)
            .margin({ bottom: HEATMAP_CELL_GAP_VP })
        }
      }, (cell: HeatmapCell) => cell.key)
    }
    .width(this.cellSizeVp)
    .margin({ right: HEATMAP_CELL_GAP_VP })
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
          .opacity(level === 0 ? 0.8 : 0.25 + level * 0.18)
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
      return { color: $r('sys.color.comp_background_gray'), opacity: 0.35, borderWidth: 0 };
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
      opacity: 0.25 + cell.intensity * 0.18,
      borderWidth: 0,
    };
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
      let monthLabel = '';
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
        if (date.getDate() === 1) {
          monthLabel = `${date.getMonth() + 1}月`;
        }
      }
      weeks.push({ key: `week-${weekIndex}`, monthLabel, days });
      cursor = new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate() + 7);
      weekIndex += 1;
    }
    return weeks;
  }

  /** 按本地日期生成 YYYY-MM-DD，避免 UTC 解析换日。 */
  private formatLocalDate(date: Date): string {
    const month = `${date.getMonth() + 1}`.padStart(2, '0');
    const day = `${date.getDate()}`.padStart(2, '0');
    return `${date.getFullYear()}-${month}-${day}`;
  }
}
