// ─── CreateTitleDialog.ets ───
// 窗口层级的标题输入对话框：新建节点 / 新建子星图共用一套 UI。
//
// 为什么挂在窗口层级：星图 Scene 是递归结构，节点菜单和对话框如果作为 Scene 的普通子组件，
// 会被圆形 Embed 的 clip(true) 裁掉，也会因为用 Scene 局部坐标定位而越出屏幕（Issue #813）。
// openCustomDialog + ComponentContent 把内容挂到窗口层级，由系统负责遮罩与居中。
//
// 为什么要单独的组件 + 全局 @Builder 函数：
// ComponentContent 的构造参数是 wrapBuilder(...)，ArkTS 要求 wrapBuilder 收到的是
// 直接的 @Builder 函数引用，不能是 `this.xxx` 形式的成员方法引用，所以这里用文件级
// @Builder 函数把参数转交给组件，组件自己持有输入状态，宿主 Scene 不再存一份标题文本。

import { UIContext, ComponentContent } from '@kit.ArkUI'
import { DesignTokens } from '../../../ui/theme/DesignTokens'

/** Issue #827 评论 1 第 7 点：命名框内容最大宽度（vp），与 HomeScreen / WorkspaceScreen 同口径。 */
const DIALOG_CONTENT_MAX_WIDTH: number = 420

/**
 * 标题输入对话框的参数。
 * 对话框与宿主 Scene 分离，通过参数告诉对话框"这次要建什么"，
 * 对话框再通过回调把结果交回 Scene。
 */
export interface CreateTitleDialogArgs {
  /** 对话框标题：新建节点 / 新建子星图 */
  title: string
  /** 输入框 placeholder */
  placeholder: string
  /** 确认回调：收到 trim 后的标题 */
  onConfirm: (title: string) => void
  /** 取消回调 */
  onCancel: () => void
}

@Component
struct CreateTitleDialog {
  /** 对话框标题 */
  @Prop dialogTitle: string = ''
  /** 输入框 placeholder */
  @Prop placeholder: string = ''
  /** 确认回调：收到 trim 后的标题 */
  onConfirm: (title: string) => void = (_title: string) => {}
  /** 取消回调 */
  onCancel: () => void = () => {}
  /** 输入内容：对话框自己持有，宿主 Scene 不存标题文本 */
  @State inputText: string = ''

  build() {
    // Issue #827 评论 1 第 7 点：命名框内容框「手机上能缩、宽屏有上限」。
    // 原来 Column 根节点写 .width('80%')，在 1244vp 宽屏上命名框被拉到近 1000vp 宽，
    // 一行输入框横跨整屏。改成 Row 铺满再放一个 100% + 上限的内容列，
    // 窄屏按屏宽收缩、宽屏停在 DIALOG_CONTENT_MAX_WIDTH。
    Row() {
      Column() {
        Text(this.dialogTitle)
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .margin({ bottom: 16 })

        TextInput({ text: this.inputText, placeholder: this.placeholder })
          .width('100%')
          .height(40)
          .fontSize(16)
          .onChange((value: string) => {
            this.inputText = value
          })

        Row() {
          Button($r('app.string.cancel'))
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.inputText = ''
              this.onCancel()
            })

          Blank()

          Button('创建')
            .fontSize(14)
            .height(36)
            .enabled(this.inputText.trim().length > 0)
            .onClick(() => {
              const title: string = this.inputText.trim()
              this.inputText = ''
              this.onConfirm(title)
            })
        }
        .width('100%')
        .margin({ top: 16 })
      }
      .width('100%')
      .constraintSize({ maxWidth: DIALOG_CONTENT_MAX_WIDTH })
      .padding(20)
      .borderRadius(DesignTokens.radiusLg)
      .backgroundColor($r('sys.color.background_secondary'))
    }
    .width('100%')
    .padding({ left: 24, right: 24 })
    .justifyContent(FlexAlign.Center)
  }
}

/** 文件级 @Builder 函数：把 ComponentContent 传来的参数转交给组件。 */
@Builder
function CreateTitleDialogBuilder(args: CreateTitleDialogArgs) {
  CreateTitleDialog({
    dialogTitle: args.title,
    placeholder: args.placeholder,
    onConfirm: args.onConfirm,
    onCancel: args.onCancel
  })
}

/**
 * 构造标题输入对话框的 ComponentContent。
 * 调用方用 getUIContext().getPromptAction().openCustomDialog(content, {...}) 打开，
 * 关闭时用同一个 content 调用 closeCustomDialog。
 *
 * @param uiContext 调用方的 UIContext
 * @param args 对话框参数
 * @returns 可用于 openCustomDialog / closeCustomDialog 的 ComponentContent
 */
export function createTitleDialogContent(
  uiContext: UIContext,
  args: CreateTitleDialogArgs
): ComponentContent<CreateTitleDialogArgs> {
  return new ComponentContent<CreateTitleDialogArgs>(uiContext, wrapBuilder(CreateTitleDialogBuilder), args)
}