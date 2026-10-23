# 内置中文纠错模型第三方声明

素笺写作的安装包内置 MacBERT4CSC 中文拼写纠错模型的 ONNX 导出文件，用于
离线、等长的字符级替换建议。模型文件不是素笺项目自制的模型，其许可与素笺
软件的 GPL-3.0-only 相互独立。本声明与
`crates/sujian-proof/THIRD_PARTY_NOTICES.md`、`docs/correction_model_assets.md`
共同构成安装包内的第三方来源与许可说明。

## 模型文件

- 模型：`macbert4csc-base-chinese`
- 上游模型仓库：<https://huggingface.co/shibing624/macbert4csc-base-chinese>
- 发布者：shibing624（Ming Xu，pycorrector 项目作者）
- 固定 revision：`615e6e09ef9a69ec487bc7c641ec3a311e2c11b9`
- 基础模型：`hfl/chinese-macbert-base`，Apache-2.0
- 上游页面许可标注：Apache-2.0
- 文件：`onnx/model.onnx`（FP32，474,336,933 字节），以及
  `vocab.txt`、`config.json`、`tokenizer.json`、`tokenizer_config.json`、
  `special_tokens_map.json`
- SHA-256 与逐文件字节数：见
  `crates/sujian-proof/assets/model_manifest.json`

素笺按上述字节数与 SHA-256 校验后原样打包，不修改模型权重。模型目录中的
`calibrator.json` 是素笺基于公开 CSC 数据训练并导出的后处理参数，属于素笺
项目数据，不改变上游模型许可。

## 许可与归属

- 模型许可全文：`third_party/macbert4csc/LICENSE`（Apache License 2.0）。
- 归属与修改说明：`third_party/macbert4csc/NOTICE`。
- 上游模型按 Apache-2.0 分发；再分发时保留版权、许可与 NOTICE，并对修改过的
  文件显著标注。素笺没有修改 ONNX 权重，只增加独立的后处理校准参数。
- 模型不重新许可为 GPL-3.0-only；素笺的 GPL-3.0-only 仅覆盖素笺自己的代码。

## ONNX Runtime

- 项目：<https://github.com/microsoft/onnxruntime>
- 许可：MIT；全文见 `third_party/onnxruntime/LICENSE`，归属见
  `third_party/onnxruntime/NOTICE`。
- Linux 包按 `assets/model_manifest.json` 锁定版本、压缩包字节数与 SHA-256，
  解包后只安装 `libonnxruntime.so`。Android 使用官方 ONNX Runtime Android
  AAR；HarmonyOS 的原生运行时需按目标 SDK 单独构建，未验证前不声明可用。
- 模型格式与运行时是相互独立的作品，ONNX Runtime 的 MIT 许可不改变模型的
  Apache-2.0 许可。

## 数据与隐私

随包只包含模型权重、词表、tokenizer 元数据和素笺校准参数，不包含训练语料。
所有推理在本机 CPU 上完成，正文和模型都不会上传。用户自行导入的模型不受
本声明约束，其许可由用户与对应来源决定。
