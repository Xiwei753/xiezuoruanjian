import { hapTasks, OhosPluginId, OhosHapContext } from '@ohos/hvigor-ohos-plugin';
import { HvigorPlugin, HvigorNode } from '@ohos/hvigor';

// 根据 product 动态设置 entry 的 dependencies：
// - default product：dependencies 为空（不依赖 privacy_acl）
// - acl product：dependencies 包含 privacy_acl（@module:privacy_acl）
// 使用官方 OhosHapContext.setDependenciesOpt API，不手改构建生成目录。
const privacyAclDependencyPlugin: HvigorPlugin = {
  pluginId: 'privacy-acl-dependency-plugin',
  apply: (node: HvigorNode) => {
    node.afterNodeEvaluate(() => {
      const hapContext = node.getContext(OhosPluginId.OHOS_HAP_PLUGIN) as OhosHapContext;
      if (!hapContext) {
        return;
      }
      hapContext.targets((target) => {
        const productName = target.getCurrentProduct().getProductName();
        if (productName === 'acl') {
          hapContext.setDependenciesOpt({ privacy_acl: '@module:privacy_acl' });
        }
        // default product: oh-package.json5 的 dependencies 已为空，无需设置
      });
    });
  }
};

export default {
  system: hapTasks,
  plugins: [privacyAclDependencyPlugin]
}
