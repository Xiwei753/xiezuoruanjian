// RootBackHandler — 顶层根页统一返回处理。
// Issue #792 评论 5884952359：
// 根页（Home/StarMap/Stats）收到系统返回键/全面屏侧滑时，不应 pop NavPathStack
// （会导致根栈 pop 空白屏），也不应单纯 return true 吞掉返回事件（会导致全面屏返回完全无反应）。
// 正确语义：隐藏左上返回按钮（.hideBackButton(true)），系统返回时退出当前 UIAbility。
// 官方说明：https://developer.huawei.com/consumer/cn/doc/doccenter-dev-faq/faqs-arkui-831

import hilog from '@ohos.hilog'
import { BusinessError } from '@kit.BasicServicesKit'
import { getAppContext } from '../AppContext'

const ROOT_NAV_TAG = 'RootNavigation'

/**
 * 顶层根页统一返回处理：退出当前 UIAbility，不 pop NavPathStack。
 *
 * 在根页的 `.onBackPressed()` 中调用，返回值始终为 `true`（表示已处理返回事件，
 * 阻止默认 pop 行为），但实际动作是 `terminateSelf()` 退出 Ability。
 *
 * @returns true — 始终返回 true，拦截默认 pop 行为
 */
export function handleRootBack(): boolean {
  const context = getAppContext().getAbilityContext()
  if (context !== null) {
    // 显式标注 catch 回调参数类型：ArkTS 的 arkts-no-any-unknown 规则禁止隐式 any。
    context.terminateSelf().catch((err: BusinessError) => {
      hilog.error(0x0000, ROOT_NAV_TAG, `terminateSelf failed: code=${err.code} msg=${err.message}`)
    })
  }
  return true
}
