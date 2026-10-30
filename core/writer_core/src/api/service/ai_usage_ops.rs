//! AI 请求输入 / 模型响应输出统计事件的业务层记录器。
//!
//! 真正的 Agent 会话（#506）在请求发出时记录一条 `Input` 事件，在响应完成时
//! 记录一条 `Output` 事件；两条事件各有稳定事件 ID、真实发生时间和设备，
//! 用同一 `conversation_id` / `turn_id` 关联。事件独立于对象修改，避免一次
//! AI 操作修改多个节点时输入/输出被重复累计。本模块不负责拉起 Agent，也不
//! 解释机器接口。

use crate::api::service::WriterCoreApi;
use crate::writing_stats::AiUsageEvent;

impl WriterCoreApi {
    /// 幂等记录一条 AI 方向事件。
    pub fn record_ai_usage_event(&self, event: AiUsageEvent) -> super::ApiResult<()> {
        self.record_ai_usage_events(vec![event]).map(|_| ())
    }

    /// 幂等记录一批 AI 方向事件。
    pub fn record_ai_usage_events(&self, events: Vec<AiUsageEvent>) -> super::ApiResult<usize> {
        // 锁序：工作区跨进程锁 → Core 写锁 → StatsApi/Store。
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        self.core_write()
            .get_stats_api()
            .record_ai_usage_events(events)
            .map_err(Into::into)
    }

    /// 读取全部已到达设备的 AI 请求输入 / 模型响应输出事实。
    pub fn load_ai_usage_events(&self) -> super::ApiResult<Vec<AiUsageEvent>> {
        // 读取也要先恢复 pending journal，锁序与写入、贡献统计查询一致。
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        self.core_read()
            .get_stats_api()
            .load_ai_usage_events()
            .map_err(Into::into)
    }
}
