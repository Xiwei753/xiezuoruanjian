// WritingStatusBar — 宽屏写作页正文底部的状态信息条（Issue #829 第二轮）。
//
// 手稿（宽屏全打开 / 左右缩回）底部都有一行状态栏，内容三段：
//   左：当前章节字数            中：进度「当前 / 目标」            右：时钟 + 保存状态
// 硬约束 6：正文底部只放字数、进度、时间、保存状态等状态信息，不承担页面导航。
//
// 空间位置：这条只属于 Editor 那一列，横贯「章节树右缘 ~ 工具面板左缘」，
// 不跨到章节树、工具面板和最右 rail 上。
//
// 数据来源（全部是真值，UI 不自己算业务状态）：
//   - chapterWordCount：Core 的 editorSnapshot/editResult 字数（WritingScreen.wordCount）。
//   - progressCurrent：Core 写作统计的当日纯输入字数（getWritingStatsSummary）。
//   - progressTarget：手稿写的是 2,000。Core 目前没有目标字数字段，
//     这一轮按用户决定先用显式常量占位，不在端侧造一个假的 Core 字段；
//     Core 加了目标字数设置后，这里改成读 Core。
//   - saveStatus：WritingScreen 的保存状态文案与颜色。
//
// 窄屏不显示这条（窄屏保留自己的布局，见 WritingScreen.EditorPane）。

/** 状态条高度（vp） */
const STATUS_BAR_HEIGHT_VP: number = 28
/** 手稿的进度分母占位：Core 尚无目标字数字段，先按手稿取 2,000 */
const PROGRESS_TARGET_PLACEHOLDER: number = 2000

@ComponentV2
export struct WritingStatusBar {
  /** Core 回流的当前章节字数 */
  @Param chapterWordCount: number = 0
  /** Core 写作统计的当日纯输入字数 */
  @Param progressCurrent: number = 0
  /** 进度分母；Core 加了目标字数设置后由外部传 Core 真值 */
  @Param progressTarget: number = PROGRESS_TARGET_PLACEHOLDER
  /** HH:mm */
  @Param clockText: string = ''
  /** 保存状态文案（已保存 / 保存中 / 未保存 / 保存失败） */
  @Param saveStatus: string = ''
  /** 保存状态是否异常——只有真失败才上警示色，"未保存"这种正常中间态不上色 */
  @Param saveStatusIsError: boolean = false

  private chapterWordText(): string {
    return `${this.chapterWordCount} 字`
  }

  private progressText(): string {
    return `${this.progressCurrent.toLocaleString()} / ${this.progressTarget.toLocaleString()}`
  }

  @Builder
  build() {
    Column() {
      // 顶部 1px 分隔线，把状态条和正文纸面分开。
      Column()
        .width('100%')
        .height(1)
        .backgroundColor($r('sys.color.comp_background_gray'))

      Row() {
        Text(this.chapterWordText())
          .fontSize(12)
          .fontColor($r('sys.color.font_secondary'))

        Blank()
          .layoutWeight(1)

        Text(this.progressText())
          .fontSize(12)
          .fontColor($r('sys.color.font_secondary'))

        Blank()
          .layoutWeight(1)

        Row() {
          Text(this.clockText)
            .fontSize(12)
            .fontColor($r('sys.color.font_secondary'))

          if (this.saveStatus.length > 0) {
            Text(this.saveStatus)
              .fontSize(12)
              .fontColor(this.saveStatusIsError ? '#E84026' : $r('sys.color.font_secondary'))
              .margin({ left: 8 })
          }
        }
        .alignItems(VerticalAlign.Center)
      }
      .width('100%')
      .height(STATUS_BAR_HEIGHT_VP)
      .padding({ left: 12, right: 12 })
      .alignItems(VerticalAlign.Center)
    }
    .width('100%')
    .height(STATUS_BAR_HEIGHT_VP + 1)
    .alignItems(HorizontalAlign.Start)
    .backgroundColor($r('sys.color.background_secondary'))
  }
}
