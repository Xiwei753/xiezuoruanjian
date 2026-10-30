//! AI 请求/响应统计事件的业务层记录器。
//!
//! 真正的 Agent 会话（#506）在请求发出、响应完成后调用这里，一次请求/响应
//! 只写一条 [`AiUsageEvent`]；事件独立于对象修改，避免一次 AI 操作修改多个
//! 节点时输入/输出被重复累计。本模块不负责拉起 Agent，也不解释机器接口。

use crate::api::service::WriterCoreApi;
use crate::writing_stats::AiUsageEvent;

impl WriterCoreApi {
    /// 幂等记录一次 AI 请求/响应事实。
    pub fn record_ai_usage_event(&self, event: AiUsageEvent) -> super::ApiResult<()> {
        self.core_write()
            .get_stats_api()
            .record_ai_usage_event(event)
            .map(|_| ())
            .map_err(Into::into)
    }

    /// 读取全部已到达设备的 AI 请求/响应事实。
    pub fn load_ai_usage_events(&self) -> super::ApiResult<Vec<AiUsageEvent>> {
        self.core_read()
            .get_stats_api()
            .load_ai_usage_events()
            .map_err(Into::into)
    }
}
