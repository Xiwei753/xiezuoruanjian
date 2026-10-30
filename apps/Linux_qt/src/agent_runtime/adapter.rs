//! Agent 适配器统一契约与工厂。
//!
//! Issue #1016：扫描器只负责发现/选择，不承载协议状态。真正的会话进程、
//! 请求序列、流式事件与取消由 [`AgentAdapter`] 的实现管理；管理器只面向
//! 这个统一接口。

use super::acp::AcpAdapter;
use super::claude::ClaudeAdapter;
use super::codex::CodexAdapter;
use super::events::{AgentEvent, AgentLaunchContext, EventSink, RuntimeEvent};
use super::gemini::GeminiAdapter;
use super::opencode::OpenCodeAdapter;

/// 一个已启动的 Agent 会话协议实现。
pub trait AgentAdapter: Send {
    /// 发送一条用户消息。实现必须尽快返回；长连接/进程读取在后台线程完成。
    ///
    /// `core_turn_id` 是本轮在 Core 中的固定身份。异步回调和读取线程必须
    /// 保存并使用这个值，不能回读会话当前轮次。
    fn prompt(&mut self, core_turn_id: &str, message: &str) -> Result<(), String>;
    /// 请求中断当前轮次。
    fn cancel(&mut self) -> Result<(), String>;
    /// 当前适配器持有的原生 session / thread ID，用于判断能否复用进程。
    fn native_session_id(&self) -> Option<String> {
        None
    }
    /// 回复 Agent 的权限请求。不支持交互式权限的协议返回明确错误。
    fn respond_permission(&mut self, _request_id: &str, _option_id: &str) -> Result<(), String> {
        Err("当前 Agent 未声明可用的权限回复接口".to_string())
    }
}

/// 按 Agent 的真实协议创建适配器。注册表标签只作提示，内置 Agent 优先使用
/// 各自官方机器接口；未知 ACP 条目才回退到通用 ACP。
pub fn create_adapter(
    ctx: &AgentLaunchContext,
    emit: EventSink,
) -> Result<Box<dyn AgentAdapter>, String> {
    // 先上报“已注入配置”。适配器启动过程中若从官方状态接口拿到真实
    // connected/failed，会在这条基线之后覆盖它；没有状态接口时不能假装
    // 连接成功。
    for server in &ctx.mcp_servers {
        (emit)(RuntimeEvent::session(AgentEvent::McpConnectionChanged {
            server: server.name.clone(),
            status: "configured".to_string(),
            error: None,
            tools: Vec::new(),
        }));
    }
    let adapter: Box<dyn AgentAdapter> = match ctx.agent_id.as_str() {
        "codex" => Box::new(CodexAdapter::start(ctx, emit.clone())?),
        "opencode" => Box::new(OpenCodeAdapter::start(ctx, emit.clone())?),
        "claude" => Box::new(ClaudeAdapter::start(ctx, emit.clone())?),
        "gemini" => Box::new(GeminiAdapter::start(ctx, emit.clone())?),
        _ if ctx.protocol == "acp" => Box::new(AcpAdapter::start(ctx, emit.clone())?),
        _ => {
            return Err(format!(
                "Agent {} 未声明可用的机器会话协议（protocol={}）",
                ctx.agent_name, ctx.protocol
            ))
        }
    };
    Ok(adapter)
}
