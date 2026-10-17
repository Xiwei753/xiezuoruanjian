// CompactDialogs — 窄屏窗口级输入 / 确认对话框的统一实现。
//
// Issue #932：作品页、星图列表、工作区各自手绘创建 / 改名 / 删除对话框
// （页面内全屏 Column + '#80000000' 遮罩 + 自绘卡片），星图列表还写死 .width('80%')。
// 统一为窗口级 openCustomDialog：遮罩、居中、返回（autoCancel）交给系统，
// 卡片外观与度量只保留这一份。
//
// 与 CreateTitleDialog 的关系：星图节点 / 子星图创建继续用窗口级容器，但外观与度量
// 改为引用本文件的公共常量与组件口径。
//
// 本文件只画对话框，不 import 任何 ViewModel / Core 服务；业务回调由调用方传入。
//
// 用法（页面只传原有回调）：
//   openCompactTextDialog(this.getUIContext(), { ...args }, () => { /* 关闭后清理 */ })
//   openCompactConfirmDialog(this.getUIContext(), { ...args })

import { UIContext, ComponentContent } from '@kit.ArkUI'
import { DesignTokens } from '../theme/DesignTokens'
import {
  COMPACT_DIALOG_CONTENT_PADDING_VP,
  COMPACT_DIALOG_MAX_WIDTH_VP,
  COMPACT_DIALOG_SIDE_INSET_VP,
  COMPACT_TEXT_INPUT_HEIGHT_VP
} from '../theme/LayoutTokens'

/** 统一遮罩色：半透明黑，与 openCustomDialog 的 maskColor 同口径。 */
export const COMPACT_DIALOG_MASK_COLOR: string = '#80000000'

/** 标题输入对话框参数。secondaryLabel 为空串时不显示第二输入栏（星图描述）。 */
export interface CompactTextDialogArgs {
  title: string
  placeholder: string
  /** 初始文本（重命名场景传入现值；新建传空串） */
  initialText: string
  /** 第二输入栏标签；空串表示不显示第二栏 */
  secondaryLabel: string
  secondaryPlaceholder: string
  secondaryInitialText: string
  /** 确认按钮文字（创建 / 保存 / 确认） */
  confirmLabel: string
  /** 确认回调：收到 trim 后的主输入与第二输入内容 */
  onConfirm: (text: string, secondaryText: string) => void
  /** 取消回调（点取消按钮触发；点遮罩由 onDidDisappear 负责） */
  onCancel: () => void
}

/** 确认对话框参数（删除等）。 */
export interface CompactConfirmDialogArgs {
  title: string
  message: string
  /** 破坏性操作：确认按钮使用警示色 */
  destructive: boolean
  /** 确认按钮文字（删除 / 退出等） */
  confirmLabel: string
  onConfirm: () => void
  onCancel: () => void
}

@Component
struct CompactTextDialog {
  @Prop dialogTitle: string = ''
  @Prop placeholder: string = ''
  @Prop initialText: string = ''
  @Prop secondaryLabel: string = ''
  @Prop secondaryPlaceholder: string = ''
  @Prop secondaryInitialText: string = ''
  @Prop confirmLabel: string = ''
  onConfirm: (text: string, secondaryText: string) => void = (_text: string, _secondaryText: string) => {}
  onCancel: () => void = () => {}
  /** 输入内容：对话框自己持有，调用方不存临时文本 */
  @State inputText: string = ''
  @State secondaryText: string = ''

  aboutToAppear(): void {
    this.inputText = this.initialText
    this.secondaryText = this.secondaryInitialText
  }

  build() {
    Row() {
      Column() {
        Text(this.dialogTitle)
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .margin({ bottom: 16 })

        TextInput({ text: this.inputText, placeholder: this.placeholder })
          .width('100%')
          .height(COMPACT_TEXT_INPUT_HEIGHT_VP)
          .fontSize(16)
          .borderRadius(DesignTokens.inputFieldRadius)
          .onChange((value: string) => {
            this.inputText = value
          })

        // 第二输入栏只在给了标签时出现（星图描述是可选业务字段）。
        if (this.secondaryLabel.length > 0) {
          Text(this.secondaryLabel)
            .fontSize(14)
            .fontColor($r('sys.color.font_secondary'))
            .margin({ top: 12, bottom: 6 })

          TextInput({ text: this.secondaryText, placeholder: this.secondaryPlaceholder })
            .width('100%')
            .height(COMPACT_TEXT_INPUT_HEIGHT_VP)
            .fontSize(16)
            .borderRadius(DesignTokens.inputFieldRadius)
            .onChange((value: string) => {
              this.secondaryText = value
            })
        }

        Row() {
          Button($r('app.string.cancel'))
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.onCancel()
            })

          Blank()

          // 空名称 trim 后统一禁用确定操作。
          Button(this.confirmLabel)
            .fontSize(14)
            .height(36)
            .enabled(this.inputText.trim().length > 0)
            .onClick(() => {
              this.onConfirm(this.inputText.trim(), this.secondaryText.trim())
            })
        }
        .width('100%')
        .margin({ top: 16 })
      }
      .width('100%')
      .constraintSize({ maxWidth: COMPACT_DIALOG_MAX_WIDTH_VP })
      .padding(COMPACT_DIALOG_CONTENT_PADDING_VP)
      .borderRadius(DesignTokens.dialogRadius)
      .backgroundColor($r('sys.color.background_secondary'))
    }
    .width('100%')
    .padding({ left: COMPACT_DIALOG_SIDE_INSET_VP, right: COMPACT_DIALOG_SIDE_INSET_VP })
    .justifyContent(FlexAlign.Center)
  }
}

@Component
struct CompactConfirmDialog {
  @Prop dialogTitle: string = ''
  @Prop message: string = ''
  @Prop destructive: boolean = false
  @Prop confirmLabel: string = ''
  onConfirm: () => void = () => {}
  onCancel: () => void = () => {}

  build() {
    Row() {
      Column() {
        Text(this.dialogTitle)
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .margin({ bottom: 8 })

        Text(this.message)
          .fontSize(14)
          .margin({ bottom: 16 })

        Row() {
          Button($r('app.string.cancel'))
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.onCancel()
            })

          Blank()

          if (this.destructive) {
            Button(this.confirmLabel)
              .fontSize(14)
              .height(36)
              .backgroundColor($r('sys.color.ohos_id_color_warning'))
              .onClick(() => {
                this.onConfirm()
              })
          } else {
            Button(this.confirmLabel)
              .fontSize(14)
              .height(36)
              .onClick(() => {
                this.onConfirm()
              })
          }
        }
        .width('100%')
        .margin({ top: 16 })
      }
      .width('100%')
      .constraintSize({ maxWidth: COMPACT_DIALOG_MAX_WIDTH_VP })
      .padding(COMPACT_DIALOG_CONTENT_PADDING_VP)
      .borderRadius(DesignTokens.dialogRadius)
      .backgroundColor($r('sys.color.background_secondary'))
    }
    .width('100%')
    .padding({ left: COMPACT_DIALOG_SIDE_INSET_VP, right: COMPACT_DIALOG_SIDE_INSET_VP })
    .justifyContent(FlexAlign.Center)
  }
}

/** 文件级 @Builder：把 ComponentContent 传入的参数转交给组件（wrapBuilder 只接受直接函数引用）。 */
@Builder
function CompactTextDialogBuilder(args: CompactTextDialogArgs) {
  CompactTextDialog({
    dialogTitle: args.title,
    placeholder: args.placeholder,
    initialText: args.initialText,
    secondaryLabel: args.secondaryLabel,
    secondaryPlaceholder: args.secondaryPlaceholder,
    secondaryInitialText: args.secondaryInitialText,
    confirmLabel: args.confirmLabel,
    onConfirm: args.onConfirm,
    onCancel: args.onCancel
  })
}

@Builder
function CompactConfirmDialogBuilder(args: CompactConfirmDialogArgs) {
  CompactConfirmDialog({
    dialogTitle: args.title,
    message: args.message,
    destructive: args.destructive,
    confirmLabel: args.confirmLabel,
    onConfirm: args.onConfirm,
    onCancel: args.onCancel
  })
}

/** 构造标题输入对话框的 ComponentContent（需要手动 open/close 时使用）。 */
export function createCompactTextDialogContent(
  uiContext: UIContext,
  args: CompactTextDialogArgs
): ComponentContent<CompactTextDialogArgs> {
  return new ComponentContent<CompactTextDialogArgs>(uiContext, wrapBuilder(CompactTextDialogBuilder), args)
}

/** 构造确认对话框的 ComponentContent（需要手动 open/close 时使用）。 */
export function createCompactConfirmDialogContent(
  uiContext: UIContext,
  args: CompactConfirmDialogArgs
): ComponentContent<CompactConfirmDialogArgs> {
  return new ComponentContent<CompactConfirmDialogArgs>(uiContext, wrapBuilder(CompactConfirmDialogBuilder), args)
}

/**
 * 打开标题输入对话框：确认 / 取消由本函数负责关闭同一 content，
 * 调用方只在回调里跑原有业务。onDidDisappear 用于点遮罩 / 返回键关闭后的页面清理。
 */
export function openCompactTextDialog(
  uiContext: UIContext,
  args: CompactTextDialogArgs,
  onDidDisappear?: () => void
): void {
  const promptAction = uiContext.getPromptAction()
  let content: ComponentContent<CompactTextDialogArgs> | null = null
  const dialogArgs: CompactTextDialogArgs = {
    title: args.title,
    placeholder: args.placeholder,
    initialText: args.initialText,
    secondaryLabel: args.secondaryLabel,
    secondaryPlaceholder: args.secondaryPlaceholder,
    secondaryInitialText: args.secondaryInitialText,
    confirmLabel: args.confirmLabel,
    onConfirm: (text: string, secondaryText: string) => {
      if (content !== null) {
        promptAction.closeCustomDialog(content)
      }
      args.onConfirm(text, secondaryText)
    },
    onCancel: () => {
      if (content !== null) {
        promptAction.closeCustomDialog(content)
      }
      args.onCancel()
    }
  }
  content = new ComponentContent<CompactTextDialogArgs>(uiContext, wrapBuilder(CompactTextDialogBuilder), dialogArgs)
  promptAction.openCustomDialog(content, {
    alignment: DialogAlignment.Center,
    maskColor: COMPACT_DIALOG_MASK_COLOR,
    autoCancel: true,
    onDidDisappear: () => {
      if (onDidDisappear !== undefined) {
        onDidDisappear()
      }
    }
  })
}

/** 打开确认对话框：确认 / 取消由本函数负责关闭同一 content。 */
export function openCompactConfirmDialog(
  uiContext: UIContext,
  args: CompactConfirmDialogArgs,
  onDidDisappear?: () => void
): void {
  const promptAction = uiContext.getPromptAction()
  let content: ComponentContent<CompactConfirmDialogArgs> | null = null
  const dialogArgs: CompactConfirmDialogArgs = {
    title: args.title,
    message: args.message,
    destructive: args.destructive,
    confirmLabel: args.confirmLabel,
    onConfirm: () => {
      if (content !== null) {
        promptAction.closeCustomDialog(content)
      }
      args.onConfirm()
    },
    onCancel: () => {
      if (content !== null) {
        promptAction.closeCustomDialog(content)
      }
      args.onCancel()
    }
  }
  content = new ComponentContent<CompactConfirmDialogArgs>(uiContext, wrapBuilder(CompactConfirmDialogBuilder), dialogArgs)
  promptAction.openCustomDialog(content, {
    alignment: DialogAlignment.Center,
    maskColor: COMPACT_DIALOG_MASK_COLOR,
    autoCancel: true,
    onDidDisappear: () => {
      if (onDidDisappear !== undefined) {
        onDidDisappear()
      }
    }
  })
}
