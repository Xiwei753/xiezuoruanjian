// SearchOverlayController — 宽屏窗口内搜索浮层的生命周期。
//
// 与 SettingsOverlayController 同一模式：窗口级 custom dialog 打开在现有页面之上，
// 关闭只 dismiss 对话框，不触碰底层 NavPathStack / 页面滚动位置。
// UIContext 由 AppNavigation 在根导航宿主注册一次。

import hilog from '@ohos.hilog'
import { UIContext, ComponentContent } from '@kit.ArkUI'
import { SearchContext } from '../../../corebridge/dto/SearchDtos'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { WideLayoutMode, getWideLayoutState } from '../../../app/navigation/wide/WideLayoutMode'
import { SearchScreen } from '../ui/SearchScreen'

const TAG = 'SearchOverlayController'
const DOMAIN = 0x0000

export interface SearchOverlayArgs {
  context: SearchContext
  initialQuery: string
  navPathStack: NavPathStack
  onRequestClose: () => void
  onNavigateAway: () => void
}

@Builder
function SearchOverlayBuilder(args: SearchOverlayArgs) {
  SearchScreen({
    navPathStack: args.navPathStack,
    overlayLayout: true,
    originContext: args.context,
    initialQuery: args.initialQuery,
    onRequestClose: args.onRequestClose,
    onNavigateAway: args.onNavigateAway
  })
}

export class SearchOverlayController {
  private static instance: SearchOverlayController | null = null
  private uiContext: UIContext | null = null
  private dialogContent: ComponentContent<SearchOverlayArgs> | null = null
  private closing: boolean = false

  static getInstance(): SearchOverlayController {
    if (!SearchOverlayController.instance) {
      SearchOverlayController.instance = new SearchOverlayController()
    }
    return SearchOverlayController.instance
  }

  attachUIContext(uiContext: UIContext): void {
    this.uiContext = uiContext
  }

  isOverlayLayout(): boolean {
    return getWideLayoutState().current() === WideLayoutMode.Wide
  }

  isOpen(): boolean {
    return this.dialogContent !== null || this.closing
  }

  open(context: SearchContext, initialQuery: string = '', onNavigateAway: () => void = (): void => {}): void {
    if (this.dialogContent !== null || this.closing) {
      return
    }
    if (!this.uiContext) {
      hilog.error(DOMAIN, TAG, 'UIContext is not attached; cannot open search overlay')
      return
    }
    const stack: NavPathStack | null = NavigationTransactionCoordinator.getInstance().getNavPathStack()
    if (stack === null) {
      hilog.error(DOMAIN, TAG, 'active NavPathStack is null; cannot open search overlay')
      return
    }

    const args: SearchOverlayArgs = {
      context: context,
      initialQuery: initialQuery,
      navPathStack: stack,
      onRequestClose: (): void => { this.close() },
      onNavigateAway: onNavigateAway
    }
    const content = new ComponentContent<SearchOverlayArgs>(
      this.uiContext, wrapBuilder(SearchOverlayBuilder), args)
    this.dialogContent = content

    this.uiContext.getPromptAction().openCustomDialog(content, {
      alignment: DialogAlignment.Center,
      maskColor: '#80000000',
      autoCancel: true,
      onDidDisappear: (): void => { this.releaseDialog(content) }
    }).catch((e: Error) => {
      this.releaseDialog(content)
      hilog.error(DOMAIN, TAG, 'openCustomDialog failed: %{public}s', String(e))
    })
  }

  close(): void {
    if (this.dialogContent === null || this.closing || !this.uiContext) {
      return
    }
    const content = this.dialogContent
    this.closing = true
    this.uiContext.getPromptAction().closeCustomDialog(content).catch((e: Error) => {
      this.closing = false
      hilog.error(DOMAIN, TAG, 'closeCustomDialog failed: %{public}s', String(e))
    })
  }

  detachUIContext(uiContext: UIContext): void {
    if (this.uiContext !== uiContext) {
      return
    }
    this.close()
    this.uiContext = null
  }

  private releaseDialog(content: ComponentContent<SearchOverlayArgs>): void {
    if (this.dialogContent !== content) {
      return
    }
    this.dialogContent = null
    this.closing = false
    content.dispose()
  }
}
