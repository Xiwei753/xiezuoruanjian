//! # Agent 会话文件存储
//!
//! Issue #1016：会话按 `agent/conversations/<id>.json` 保存到 Core 应用数据
//! 根目录，和项目、统计、设置走同一个工作区与同一套原子写入规则。平台端
//! 只通过 Core API 读写，不直接操作这些文件。

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::storage::{atomic_write_string, WorkspaceWriteLock};

use super::conversation::{AgentConversation, AgentConversationStatus};

const CONVERSATIONS_DIR: &str = "agent/conversations";

/// 会话 ID 必须是 Core 生成的 UUID 或外部导入的安全标识，禁止路径分隔符。
fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(Error::InvalidDeleteTarget(format!(
            "invalid agent conversation id: {id}"
        )));
    }
    Ok(())
}

pub struct AgentConversationStore {
    app_data_root: PathBuf,
}

impl AgentConversationStore {
    pub fn new(app_data_root: &Path) -> Self {
        Self {
            app_data_root: app_data_root.to_path_buf(),
        }
    }

    fn conversations_dir(&self) -> PathBuf {
        self.app_data_root.join(CONVERSATIONS_DIR)
    }

    fn path_for(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self.conversations_dir().join(format!("{id}.json")))
    }

    /// 原子保存整个会话。工作区跨进程写锁保证与其它 Core 写入串行。
    pub fn save(&self, conversation: &AgentConversation) -> Result<()> {
        validate_id(&conversation.id)?;
        let _workspace_lock = WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let directory = self.conversations_dir();
        fs::create_dir_all(&directory)?;
        let content = serde_json::to_string_pretty(conversation)?;
        atomic_write_string(&self.path_for(&conversation.id)?, &content)
    }

    pub fn load(&self, id: &str) -> Result<Option<AgentConversation>> {
        let path = self.path_for(id)?;
        match fs::read_to_string(&path) {
            Ok(content) => {
                let mut conversation = serde_json::from_str::<AgentConversation>(&content)?;
                if conversation.migrate_legacy() {
                    self.save(&conversation)?;
                }
                Ok(Some(conversation))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// 读取某个作品的会话摘要（完整会话对象，按更新时间倒序）。传入 `None`
    /// 时返回所有作品。单个损坏文件被跳过，不阻塞其它会话。
    pub fn list(&self, project_id: Option<&str>) -> Result<Vec<AgentConversation>> {
        let directory = self.conversations_dir();
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut conversations = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(mut conversation) = serde_json::from_str::<AgentConversation>(&content) else {
                log::warn!(
                    "[agent] skipping unreadable conversation file {}",
                    path.display()
                );
                continue;
            };
            conversation.migrate_legacy();
            if let Some(project_id) = project_id {
                if conversation.project_id != project_id {
                    continue;
                }
            }
            conversations.push(conversation);
        }
        conversations.sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
        Ok(conversations)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let path = self.path_for(id)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// 切换作品时把仍在运行的旧会话收口为 failed，避免重启后永远显示运行中。
    pub fn mark_project_conversations_idle(&self, project_id: &str, now_ms: i64) -> Result<()> {
        let conversations = self.list(Some(project_id))?;
        for mut conversation in conversations {
            if conversation.status == AgentConversationStatus::Running {
                conversation.status = AgentConversationStatus::Failed;
                conversation.updated_at_ms = now_ms;
                self.save(&conversation)?;
            }
        }
        Ok(())
    }
}
