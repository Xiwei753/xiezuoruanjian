import { hapTasks, OhosPluginId, OhosAppContext, OhosHapContext } from '@ohos/hvigor-ohos-plugin';
import { HvigorPlugin, HvigorNode } from '@ohos/hvigor';
import { execFileSync } from 'child_process';
import { copyFileSync, existsSync, mkdirSync, writeFileSync } from 'fs';
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

// CMake must see the same libc++_shared.so whether the build starts through
// tools/build_harmony.sh or directly through DevEco/hvigor. Run the shared
// SDK-to-prebuilt copy step before each active native target configures CMake.
const harmonyNativeRuntimePlugin: HvigorPlugin = {
  pluginId: 'harmony-native-runtime-plugin',
  apply: (node: HvigorNode) => {
    node.afterNodeEvaluate(() => {
      const hapContext = node.getContext(OhosPluginId.OHOS_HAP_PLUGIN) as OhosHapContext;
      if (!hapContext) {
        return;
      }

      const nativeBuildTasks: string[] = [];
      hapContext.targets((target) => {
        nativeBuildTasks.push(`${target.getTargetName()}@BuildNativeWithCmake`);
      });

      node.registerTask({
        name: 'PrepareHarmonyNativeRuntime',
        // postDependencies makes BuildNativeWithCmake depend on this preparation task.
        postDependencies: nativeBuildTasks,
        run: (taskContext) => {
          const projectNode = node.getParentNode();
          const projectContext = projectNode?.getContext(OhosPluginId.OHOS_APP_PLUGIN) as OhosAppContext | undefined;
          if (!projectContext) {
            throw new Error('Unable to resolve the HarmonyOS SDK selected by hvigor.');
          }
          const sdkDir = projectContext.getSdkDetails().getSdkDir();
          const nativeCandidates = [
            resolve(sdkDir, 'default', 'openharmony', 'native'),
            resolve(sdkDir, 'openharmony', 'native'),
          ];
          const nativeDir = nativeCandidates.find((candidate) => existsSync(resolve(
            candidate,
            'llvm', 'lib', 'aarch64-linux-ohos', 'c++', 'libc++_shared.so'
          )));
          if (!nativeDir) {
            throw new Error(`HarmonyOS Native SDK was not found below hvigor SDK directory: ${sdkDir}`);
          }
          const repoRoot = resolve(taskContext.modulePath, '..', '..', '..');
          const prebuiltDir = resolve(taskContext.modulePath, 'src', 'main', 'prebuilt', 'arm64-v8a');
          mkdirSync(prebuiltDir, { recursive: true });
          // CMake checks for this library during configure, so the native task
          // materializes the manifest-pinned HarmonyOS runtime before the SDK
          // libc++ copy and before BuildNativeWithCmake starts.
          execFileSync('python3', [
            resolve(repoRoot, 'crates', 'sujian-proof', 'tools', 'fetch_model.py'),
            '--runtime', 'harmony-arm64',
            '--runtime-only',
            '--runtime-output', prebuiltDir,
          ], { stdio: 'inherit' });
          execFileSync('bash', [
            resolve(repoRoot, 'tools', 'prepare_harmony_native_runtime.sh'),
            nativeDir,
            prebuiltDir,
          ], { stdio: 'inherit' });
        }
      });
    });
  }
};

// Issue #1007：把实际构建的 Git revision 写进 ArkTS 诊断身份。
// 每次 CompileArkTS 前生成文件，DevEco Studio 与命令行 hvigor 构建走同一路径。
// 文件是构建生成物，不入库；git 不可用时明确写 unknown，不能伪造版本。
const buildRevisionPlugin: HvigorPlugin = {
  pluginId: 'build-revision-plugin',
  apply: (node: HvigorNode) => {
    node.afterNodeEvaluate(() => {
      const hapContext = node.getContext(OhosPluginId.OHOS_HAP_PLUGIN) as OhosHapContext;
      if (!hapContext) {
        return;
      }
      const compileArkTsTasks: string[] = [];
      hapContext.targets((target) => {
        compileArkTsTasks.push(`${target.getTargetName()}@CompileArkTS`);
      });
      node.registerTask({
        name: 'GenerateBuildRevision',
        postDependencies: compileArkTsTasks,
        run: (taskContext) => {
          const repoRoot = resolve(taskContext.modulePath, '..', '..', '..');
          let revision = 'unknown';
          try {
            revision = String(execFileSync('git', ['rev-parse', '--short', 'HEAD'], {
              cwd: repoRoot,
              encoding: 'utf8'
            })).trim();
            if (revision.length === 0) {
              revision = 'unknown';
            } else {
              const dirty = String(execFileSync('git', ['status', '--porcelain', '--untracked-files=no'], {
                cwd: repoRoot,
                encoding: 'utf8'
              })).trim();
              if (dirty.length > 0) {
                revision += '-dirty';
              }
            }
          } catch (_) {
            revision = 'unknown';
          }
          const generatedDir = resolve(taskContext.modulePath, 'src', 'main', 'ets', 'generated');
          mkdirSync(generatedDir, { recursive: true });
          writeFileSync(
            resolve(generatedDir, 'BuildRevision.ets'),
            `// Generated by entry/hvigorfile.ts; do not edit.\nexport const BUILD_GIT_SHA: string = '${revision}'\n`,
            'utf8'
          );
        }
      });
    });
  }
};

export default {
  system: hapTasks,
  plugins: [privacyAclDependencyPlugin, bundledCorrectionModelPlugin, harmonyNativeRuntimePlugin, buildRevisionPlugin]
}
