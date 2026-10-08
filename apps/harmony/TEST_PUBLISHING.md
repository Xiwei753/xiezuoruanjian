# HarmonyOS 邀请测试自动送审

GitHub Actions 工作流：`.github/workflows/harmony_test_publish.yml`。

- 仅在 HarmonyOS 源码、共享内核相关路径变更或手动触发时运行；不因纯 Android/Qt 提交而送审。
- 构建签名 APP，验证签名后上传 AGC；等待华为软件包编译成功，再关联测试群组、更新版本信息。
- **只有新版安装包已准备完成，才进入清理阶段**：查询旧测试版本（仅发布类型 6），取得各版本详细状态并验证；草稿直接删除；正在审核的先撤销审核；已在测或待生效的先停止测试；随后删除旧版。
- 所有旧版本均成功删除后，调用华为测试版本审核提交接口；审核占位尚未释放时，对同一个新版 versionId 进行有限次数重试，绝不重复创建新版。
- 旧版属于“预审中”或 API 返回未知状态、撤销/停止/删除失败时，任务会明确失败，**不提交新版**。这是保护措施，避免盲目删除正式版本。
- 此流程会停止旧测试版本，**旧邀请链接将暂时无法用来安装旧包**；新版需等待华为审核通过且进入测试时间后才能通过 AppTest 链接安装。

华为相关接口：`/publish/v3/version/brief-info/list`、`/publish/v3/app-info`、`/publish/v3/version/on-shelf/cancel`、`/publish/v2/test/version/stop`、`/publish/v2/test/app/version`（DELETE）、`/publish/v2/test/app/version/submit`。

> 注意：编译与签名通过不等于华为审核通过。只有日志出现 `Submitted test version for review` 才代表华为已接受送审请求；审核结果以 AGC 后台为准。
