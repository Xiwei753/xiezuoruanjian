import { hapTasks, OhosPluginId, OhosHapContext } from '@ohos/hvigor-ohos-plugin';
import { HvigorPlugin, HvigorNode } from '@ohos/hvigor';
import { execFileSync } from 'child_process';
import { copyFileSync, mkdirSync } from 'fs';
import { resolve } from 'path';

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

// Issue #985：内置中文 MacBERT4CSC ONNX 模型与第三方许可/来源文本随 entry HAP 打包。
// 在资源编译（CompileResource）之前调用 packaging/correction/fetch-model.sh
// 按 model_manifest.json 获取并校验 ONNX 模型（摘要一致的已有文件直接复用），并把
// 许可与来源声明复制进 rawfile 资源目录；DevEco Studio、命令行 hvigorw
// assembleHap 与发布流水线等所有 HAP 构建入口都会先经过本任务，干净工作区
// 也能自动得到同一份资源。模型二进制不入库（.gitignore 忽略生成物）。
const bundledCorrectionModelPlugin: HvigorPlugin = {
  pluginId: 'bundled-correction-model-plugin',
  apply: (node: HvigorNode) => {
    node.afterNodeEvaluate(() => {
      const hapContext = node.getContext(OhosPluginId.OHOS_HAP_PLUGIN) as OhosHapContext;
      if (!hapContext) {
        return;
      }
      // 只对本次构建启用的 target 声明资源编译后置依赖：hapContext.targets 返回的
      // 列表已由模块任务服务按当前 product 过滤（default 构建只有 default，acl 产品
      // 构建只有 acl），因此两个方向都不会出现 “postDependency ... not found” 警告。
      const compileResourceTasks: string[] = [];
      hapContext.targets((target) => {
        compileResourceTasks.push(`${target.getTargetName()}@CompileResource`);
      });
      node.registerTask({
        name: 'PrepareBundledCorrectionModel',
        // 资源编译前必须完成：任何会打包 resources/rawfile 的任务都先执行本任务。
        postDependencies: compileResourceTasks,
        run: (taskContext) => {
          const repoRoot = resolve(taskContext.modulePath, '..', '..', '..');
          const correctionDir = resolve(repoRoot, 'packaging', 'correction');
          const rawfileDir = resolve(taskContext.modulePath, 'src', 'main', 'resources', 'rawfile');
          mkdirSync(rawfileDir, { recursive: true });
          execFileSync('bash', [resolve(correctionDir, 'fetch-model.sh'), rawfileDir], { stdio: 'inherit' });
          copyFileSync(resolve(correctionDir, 'THIRD_PARTY_MODEL_NOTICES.md'),
            resolve(rawfileDir, 'THIRD_PARTY_MODEL_NOTICES.md'));
          copyFileSync(resolve(repoRoot, 'third_party', 'macbert4csc', 'LICENSE'),
            resolve(rawfileDir, 'MACBERT4CSC-Apache-2.0.txt'));
          copyFileSync(resolve(repoRoot, 'third_party', 'onnxruntime', 'LICENSE'),
            resolve(rawfileDir, 'ONNXRUNTIME-MIT.txt'));
        }
      });
    });
  }
};

export default {
  system: hapTasks,
  plugins: [privacyAclDependencyPlugin, bundledCorrectionModelPlugin]
}
