//! # 真实写作会话计时
//!
//! 本模块只回答一个问题：**用户在前台编辑器里实际停留了多久**。
//!
//! 输入事件（[`crate::writing_stats::WritingInputEvent`]）仍然是「输入了多少字」
//! 的唯一事实源；它不携带可信时长。老字段 `duration_seconds` / `session_id`
//! 不能再拿来当计时依据。真实计时由平台端观察前台/路由事实后，按单调时钟
//! 累积成 [`WritingActivitySegment`]，再由 Core 按 `event_id` 幂等持久化。
//!
//! ## 计时语义
//!
//! - **开始**：正文编辑器处于前台且可交互，第一次人工输入落地时开始计时；
//! - **继续**：只要编辑器仍在前台，思考、停顿、删除都算写作时间；
//! - **暂停**：切应用、最小化、锁屏、进入后台、离开正文页或用户手动暂停时，
//!   立即结束当前前台切片；
//! - **恢复**：回到前台后不补算离开的秒数，等下一次人工输入或用户显式继续；
//! - **结束**：关闭正文/退出应用时结束会话；进程没运行时的时间永远不补记。
//!
//! [`WritingSessionClock`] 是平台端可以直接复用的状态机。它只接受调用方传入的
//! 单调毫秒（例如 Qt `QElapsedTimer` / Android `SystemClock.elapsedRealtime` /
//! Harmony 的单调时钟），不拿墙钟差值算时长；墙钟只用于把切片锚定到本地日期。

use crate::error::{Error, Result};
use crate::writing_stats::calendar;
use crate::writing_stats::Platform;
use chrono::{DateTime, Local, LocalResult, TimeZone};
use serde::{Deserialize, Serialize};

/// 平台端定期把已完成切片交给 Core 的建议间隔（毫秒）。
pub const WRITING_SEGMENT_FLUSH_MS: i64 = 30_000;

/// 单个切片的最大时长。超过一天说明平台时钟或状态机出了错，直接拒绝，
/// 避免一条坏数据污染统计库。
pub const MAX_WRITING_SEGMENT_MS: i64 = 24 * 60 * 60 * 1000;

/// 平台端暂停/结束计时的原因。存成字符串以兼容未来平台新增原因。
pub const PAUSE_REASON_BACKGROUND: &str = "background";
pub const PAUSE_REASON_WINDOW_INACTIVE: &str = "windowInactive";
pub const PAUSE_REASON_LEFT_EDITOR: &str = "leftEditor";
pub const PAUSE_REASON_MANUAL: &str = "manual";
pub const PAUSE_REASON_LOCK: &str = "lock";
pub const PAUSE_REASON_CLOSE: &str = "close";
pub const PAUSE_REASON_HEARTBEAT: &str = "heartbeat";
pub const PAUSE_REASON_CHAPTER_SWITCH: &str = "chapterSwitch";
pub const PAUSE_REASON_UNKNOWN: &str = "unknown";

/// 一段**已经测量完成**的前台编辑器计时切片。
///
/// 它不是一个会话的首末输入时间差，而是平台端在真实前台状态下用单调时钟
/// 累计出来的时长。`start_at_ms` / `end_at_ms` 是墙钟时间，只用于本地日期
/// 归属和与输入事件对时；`duration_ms` 是实际计时长度。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WritingActivitySegment {
    /// 平台端固定并原样重发的事件 ID；Core 按它幂等去重。
    pub event_id: String,
    /// 写作设备身份。由 Core 从持久化设备信息填入，平台端不传。
    pub device_id: String,
    pub platform: Platform,
    #[serde(default)]
    pub device_class: String,
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    /// 平台端本次写作会话 ID；同一次前台写作会话的多个切片共享它。
    pub session_id: String,
    /// 切片开始时写作设备本地的业务日（`YYYY-MM-DD`）。
    #[serde(default)]
    pub local_date: String,
    /// 切片墙钟起点（Unix 毫秒）。
    pub start_at_ms: i64,
    /// 切片墙钟终点（Unix 毫秒）。
    pub end_at_ms: i64,
    /// 实际计时时长（毫秒）。正常情况下等于 `end_at_ms - start_at_ms`。
    pub duration_ms: u64,
    /// 本切片结束的原因（[`PAUSE_REASON_*`]）。
    #[serde(default)]
    pub pause_reason: String,
}

impl WritingActivitySegment {
    /// 创建一条切片。`event_id` 为空时生成 UUID；`local_date` 由 Core 按起点
    /// 所在本地日计算，平台端无法伪造归属日。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        event_id: String,
        device_id: &str,
        platform: Platform,
        device_class: &str,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        session_id: &str,
        start_at_ms: i64,
        end_at_ms: i64,
        pause_reason: &str,
    ) -> Result<Self> {
        validate_span(start_at_ms, end_at_ms)?;
        let event_id = if event_id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            event_id
        };
        let local_date = calendar::local_date_at(start_at_ms)
            .or_else(|_| calendar::utc_date_at(start_at_ms))?;
        Ok(Self {
            event_id,
            device_id: device_id.to_string(),
            platform,
            device_class: device_class.to_string(),
            project_id: project_id.to_string(),
            volume_id: volume_id.to_string(),
            chapter_id: chapter_id.to_string(),
            session_id: session_id.to_string(),
            local_date,
            start_at_ms,
            end_at_ms,
            duration_ms: u64::try_from(end_at_ms - start_at_ms).unwrap_or(0),
            pause_reason: normalized_reason(pause_reason).to_string(),
        })
    }

    /// 切片归属的业务日历日。新切片直接读持久化的 `local_date`；老数据
    /// （字段缺失）回退到按起点时间现算。
    pub fn business_date(&self) -> String {
        if !self.local_date.is_empty() {
            return self.local_date.clone();
        }
        calendar::local_date_at(self.start_at_ms).unwrap_or_default()
    }

    /// 把跨越本地午夜的切片按写作设备本地日期切开。
    ///
    /// 每条派生切片使用稳定的 `"{event_id}@{local_date}"`：同一输入重放时得到
    /// 同一组 ID，Core 幂等去重不会重复累计时长。
    pub fn split_by_local_date(&self) -> Result<Vec<Self>> {
        validate_span(self.start_at_ms, self.end_at_ms)?;
        let mut slices = Vec::new();
        let mut cursor = self.start_at_ms;
        while cursor < self.end_at_ms {
            let date = calendar::local_date_at(cursor)
                .or_else(|_| calendar::utc_date_at(cursor))?;
            let next_midnight = next_local_midnight_ms(cursor)?.unwrap_or(self.end_at_ms);
            let slice_end = next_midnight.min(self.end_at_ms);
            if slice_end <= cursor {
                return Err(Error::Other(format!(
                    "writing activity segment did not advance at {cursor}"
                )));
            }
            let mut slice = self.clone();
            slice.event_id = format!("{}@{}", self.event_id, date);
            slice.local_date = date;
            slice.start_at_ms = cursor;
            slice.end_at_ms = slice_end;
            slice.duration_ms = u64::try_from(slice_end - cursor).unwrap_or(0);
            slices.push(slice);
            cursor = slice_end;
        }
        Ok(slices)
    }
}

fn validate_span(start_at_ms: i64, end_at_ms: i64) -> Result<()> {
    if start_at_ms <= 0 || end_at_ms <= 0 {
        return Err(Error::Other(
            "writing activity segment timestamps must be positive".to_string(),
        ));
    }
    if end_at_ms < start_at_ms {
        return Err(Error::Other(format!(
            "writing activity segment end {} is before start {}",
            end_at_ms, start_at_ms
        )));
    }
    if end_at_ms - start_at_ms > MAX_WRITING_SEGMENT_MS {
        return Err(Error::Other(format!(
            "writing activity segment too long: {} ms",
            end_at_ms - start_at_ms
        )));
    }
    Ok(())
}

fn normalized_reason(reason: &str) -> &str {
    if reason.is_empty() {
        PAUSE_REASON_UNKNOWN
    } else {
        reason
    }
}

/// 本地下一个午夜（Unix 毫秒）。夏令时导致不存在/重复的午夜按最接近的
/// 有效时刻处理；只有日期超出可表示范围才返回 `None`。
fn next_local_midnight_ms(timestamp_ms: i64) -> Result<Option<i64>> {
    let Some(local) = DateTime::from_timestamp_millis(timestamp_ms) else {
        return Err(Error::Other(format!(
            "invalid writing activity timestamp: {timestamp_ms}"
        )));
    };
    let local = local.with_timezone(&Local);
    let Some(next_date) = local.date_naive().succ_opt() else {
        return Ok(None);
    };
    let Some(naive) = next_date.and_hms_opt(0, 0, 0) else {
        return Ok(None);
    };
    Ok(match Local.from_local_datetime(&naive) {
        LocalResult::Single(value) => Some(value.timestamp_millis()),
        LocalResult::Ambiguous(first, second) => {
            Some(first.timestamp_millis().min(second.timestamp_millis()))
        }
        LocalResult::None => None,
    })
}

/// 计时状态。只有 `Writing` 状态会产生时长。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritingSessionState {
    Idle,
    Writing,
    Paused,
}

/// 平台端可复用的写作会话时钟。
///
/// 所有时长都用调用方传入的**单调毫秒**计算；`wall_ms` 只在会话第一次人工
/// 输入时锚定一次，用来把切片写回真实日期。这样应用运行期间系统改时间也不会
/// 让正在进行的计时忽长忽短。
#[derive(Debug)]
pub struct WritingSessionClock {
    state: WritingSessionState,
    session_id: String,
    project_id: String,
    volume_id: String,
    chapter_id: String,
    /// 当前前台切片的单调起点。
    segment_start_mono_ms: i64,
    /// 当前会话第一次计时时的墙钟锚点。
    wall_anchor_ms: i64,
    mono_anchor_ms: i64,
    /// 下一次切片开始时的暂停原因（只用于诊断，不参与时长）。
    last_pause_reason: String,
}

impl Default for WritingSessionClock {
    fn default() -> Self {
        Self::new()
    }
}

impl WritingSessionClock {
    pub fn new() -> Self {
        Self {
            state: WritingSessionState::Idle,
            session_id: String::new(),
            project_id: String::new(),
            volume_id: String::new(),
            chapter_id: String::new(),
            segment_start_mono_ms: 0,
            wall_anchor_ms: 0,
            mono_anchor_ms: 0,
            last_pause_reason: String::new(),
        }
    }

    pub fn state(&self) -> WritingSessionState {
        self.state
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// 第一次人工输入 / 暂停后再次输入：开始一个新的前景切片。
    ///
    /// 回到前台本身不调用这里，也绝不把「上次输入到这次恢复」的离线时间补回来。
    pub fn on_first_human_edit(
        &mut self,
        mono_ms: i64,
        wall_ms: i64,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        device_id: &str,
        platform: Platform,
        device_class: &str,
    ) -> Vec<WritingActivitySegment> {
        let mut completed = Vec::new();
        match self.state {
            WritingSessionState::Writing => {
                // 同一会话里切换章节：结束旧切片，避免时长记到错误章节。
                if self.project_id != project_id
                    || self.volume_id != volume_id
                    || self.chapter_id != chapter_id
                {
                    completed.extend(self.take_segment(
                        mono_ms,
                        PAUSE_REASON_CHAPTER_SWITCH,
                        device_id,
                        platform,
                        device_class,
                    ));
                    self.begin_slice(
                        mono_ms,
                        project_id,
                        volume_id,
                        chapter_id,
                    );
                }
            }
            WritingSessionState::Idle => {
                self.session_id = uuid::Uuid::new_v4().to_string();
                self.wall_anchor_ms = wall_ms;
                self.mono_anchor_ms = mono_ms;
                self.begin_slice(mono_ms, project_id, volume_id, chapter_id);
            }
            WritingSessionState::Paused => {
                // 暂停后第一次输入才恢复计时；不补算暂停期间。
                self.begin_slice(mono_ms, project_id, volume_id, chapter_id);
            }
        }
        completed
    }

    /// 显式「继续计时」：用户在前台确认继续后，即使还没输入也重新开始累计。
    /// 没有已暂停的会话时返回空。
    pub fn on_explicit_continue(&mut self, mono_ms: i64) -> bool {
        if self.state != WritingSessionState::Paused || self.session_id.is_empty() {
            return false;
        }
        self.begin_slice(
            mono_ms,
            &self.project_id.clone(),
            &self.volume_id.clone(),
            &self.chapter_id.clone(),
        );
        true
    }

    /// 周期性心跳。达到建议切片间隔时输出一个完整切片，不结束会话。
    pub fn on_heartbeat(
        &mut self,
        mono_ms: i64,
        device_id: &str,
        platform: Platform,
        device_class: &str,
    ) -> Vec<WritingActivitySegment> {
        if self.state != WritingSessionState::Writing {
            return Vec::new();
        }
        if mono_ms - self.segment_start_mono_ms < WRITING_SEGMENT_FLUSH_MS {
            return Vec::new();
        }
        self.take_segment(
            mono_ms,
            PAUSE_REASON_HEARTBEAT,
            device_id,
            platform,
            device_class,
        )
        .into_iter()
        .collect()
    }

    /// 切应用 / 最小化 / 锁屏 / 离开正文 / 手动暂停：立即结束当前切片。
    pub fn on_pause(
        &mut self,
        mono_ms: i64,
        reason: &str,
        device_id: &str,
        platform: Platform,
        device_class: &str,
    ) -> Vec<WritingActivitySegment> {
        if self.state != WritingSessionState::Writing {
            return Vec::new();
        }
        let completed = self.take_segment(mono_ms, reason, device_id, platform, device_class);
        self.state = WritingSessionState::Paused;
        self.last_pause_reason = normalized_reason(reason).to_string();
        completed.into_iter().collect()
    }

    /// 回到前台。只解除暂停标记，等待下一次输入或显式继续；不产生时长。
    pub fn on_resume(&mut self) {
        // 状态保持 Paused：下一次 on_first_human_edit / on_explicit_continue 才开始计时。
        self.last_pause_reason.clear();
    }

    /// 关闭正文/退出应用：结束当前会话。重复调用不会重复输出切片。
    pub fn on_close(
        &mut self,
        mono_ms: i64,
        device_id: &str,
        platform: Platform,
        device_class: &str,
    ) -> Vec<WritingActivitySegment> {
        if self.state == WritingSessionState::Writing {
            let completed = self.take_segment(
                mono_ms,
                PAUSE_REASON_CLOSE,
                device_id,
                platform,
                device_class,
            );
            self.reset();
            completed.into_iter().collect()
        } else {
            self.reset();
            Vec::new()
        }
    }

    /// 强制刷出当前切片但保持会话继续；用于退出前或手动 flush。
    pub fn flush(
        &mut self,
        mono_ms: i64,
        reason: &str,
        device_id: &str,
        platform: Platform,
        device_class: &str,
    ) -> Vec<WritingActivitySegment> {
        if self.state != WritingSessionState::Writing {
            return Vec::new();
        }
        self.take_segment(mono_ms, reason, device_id, platform, device_class)
            .into_iter()
            .collect()
    }

    fn begin_slice(
        &mut self,
        mono_ms: i64,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) {
        self.state = WritingSessionState::Writing;
        self.project_id = project_id.to_string();
        self.volume_id = volume_id.to_string();
        self.chapter_id = chapter_id.to_string();
        self.segment_start_mono_ms = mono_ms;
    }

    fn take_segment(
        &mut self,
        mono_ms: i64,
        reason: &str,
        device_id: &str,
        platform: Platform,
        device_class: &str,
    ) -> Option<WritingActivitySegment> {
        let duration_ms = mono_ms.saturating_sub(self.segment_start_mono_ms);
        if duration_ms <= 0 || self.session_id.is_empty() {
            self.segment_start_mono_ms = mono_ms;
            return None;
        }
        let start_at_ms = self.wall_anchor_ms + (self.segment_start_mono_ms - self.mono_anchor_ms);
        let end_at_ms = self.wall_anchor_ms + (mono_ms - self.mono_anchor_ms);
        self.segment_start_mono_ms = mono_ms;
        WritingActivitySegment::new(
            String::new(),
            device_id,
            platform,
            device_class,
            &self.project_id,
            &self.volume_id,
            &self.chapter_id,
            &self.session_id,
            start_at_ms,
            end_at_ms,
            reason,
        )
        .ok()
    }

    fn reset(&mut self) {
        self.state = WritingSessionState::Idle;
        self.session_id.clear();
        self.project_id.clear();
        self.volume_id.clear();
        self.chapter_id.clear();
        self.segment_start_mono_ms = 0;
        self.wall_anchor_ms = 0;
        self.mono_anchor_ms = 0;
        self.last_pause_reason.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(clock: &mut WritingSessionClock, mono: i64, wall: i64) -> Vec<WritingActivitySegment> {
        clock.on_first_human_edit(
            mono,
            wall,
            "project",
            "volume",
            "chapter",
            "device",
            Platform::Desktop,
            "desktop",
        )
    }

    fn segments(clock: &mut WritingSessionClock, mono: i64) -> Vec<WritingActivitySegment> {
        clock.on_heartbeat(mono, "device", Platform::Desktop, "desktop")
    }

    #[test]
    fn clock_counts_thinking_and_flushes_every_interval() {
        let mut clock = WritingSessionClock::new();
        assert!(edit(&mut clock, 1_000, 1_000).is_empty());
        // 30 秒纯思考，没有输入，心跳仍然计入。
        let flushed = segments(&mut clock, 31_000);
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].duration_ms, 30_000);
        assert_eq!(flushed[0].pause_reason, PAUSE_REASON_HEARTBEAT);
    }

    #[test]
    fn clock_pause_and_resume_never_backfills() {
        let mut clock = WritingSessionClock::new();
        edit(&mut clock, 0, 10_000);
        let paused = clock.on_pause(
            10_000,
            PAUSE_REASON_BACKGROUND,
            "device",
            Platform::Desktop,
            "desktop",
        );
        assert_eq!(paused[0].duration_ms, 10_000);
        // 离开 10 分钟后回到前台：不产生时长。
        clock.on_resume();
        assert!(segments(&mut clock, 610_000).is_empty());
        // 下一次输入才开新切片。
        edit(&mut clock, 620_000, 630_000);
        let flushed = segments(&mut clock, 650_000);
        assert_eq!(flushed[0].duration_ms, 30_000);
    }

    #[test]
    fn clock_repeated_pause_and_close_do_not_double_count() {
        let mut clock = WritingSessionClock::new();
        edit(&mut clock, 0, 10_000);
        let first = clock.on_pause(
            5_000,
            PAUSE_REASON_BACKGROUND,
            "device",
            Platform::Desktop,
            "desktop",
        );
        assert_eq!(first.len(), 1);
        assert!(clock
            .on_pause(
                6_000,
                PAUSE_REASON_BACKGROUND,
                "device",
                Platform::Desktop,
                "desktop"
            )
            .is_empty());
        assert!(clock
            .on_close(7_000, "device", Platform::Desktop, "desktop")
            .is_empty());
    }

    #[test]
    fn clock_close_ends_session_without_backfilling_on_reopen() {
        let mut clock = WritingSessionClock::new();
        edit(&mut clock, 0, 10_000);
        let closed = clock.on_close(5_000, "device", Platform::Desktop, "desktop");
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].duration_ms, 5_000);
        assert_eq!(clock.state(), WritingSessionState::Idle);
        // 5 分钟后重新打开正文：第一次输入开启全新会话，不能补算离开时间。
        assert!(edit(&mut clock, 305_000, 315_000).is_empty());
        let flushed = segments(&mut clock, 335_000);
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].duration_ms, 30_000);
        assert_ne!(flushed[0].session_id, closed[0].session_id);
    }

    #[test]
    fn segment_split_by_local_date_moves_each_slice_start() {
        let base = 1_700_000_000_000;
        let midnight = next_local_midnight_ms(base).unwrap().unwrap();
        let start = midnight - 60_000;
        let end = midnight + 60_000;
        let segment = WritingActivitySegment::new(
            "segment-midnight".to_string(),
            "device",
            Platform::Desktop,
            "desktop",
            "project",
            "volume",
            "chapter",
            "session",
            start,
            end,
            PAUSE_REASON_CLOSE,
        )
        .unwrap();
        let slices = segment.split_by_local_date().unwrap();
        assert_eq!(slices.len(), 2);
        assert_eq!(slices[0].start_at_ms, start);
        assert_eq!(slices[0].end_at_ms, midnight);
        assert_eq!(slices[1].start_at_ms, midnight);
        assert_eq!(slices[1].end_at_ms, end);
        assert_eq!(
            slices.iter().map(|slice| slice.duration_ms).sum::<u64>(),
            120_000
        );
        assert_ne!(slices[0].local_date, slices[1].local_date);
    }

    #[test]
    fn segment_split_by_local_date_uses_stable_ids() {
        let single = WritingActivitySegment::new(
            "segment-2".to_string(),
            "device",
            Platform::Desktop,
            "desktop",
            "project",
            "volume",
            "chapter",
            "session",
            1_700_000_000_000,
            1_700_000_060_000,
            PAUSE_REASON_CLOSE,
        )
        .unwrap();
        let slices = single.split_by_local_date().unwrap();
        assert_eq!(slices.len(), 1);
        assert!(slices[0].event_id.starts_with("segment-2@"));
        // 超过一天说明平台时钟错误，创建阶段就必须拒绝。
        assert!(WritingActivitySegment::new(
            "segment-1".to_string(),
            "device",
            Platform::Desktop,
            "desktop",
            "project",
            "volume",
            "chapter",
            "session",
            1_700_000_000_000,
            1_700_000_000_000 + 26 * 60 * 60 * 1000,
            PAUSE_REASON_CLOSE,
        )
        .is_err());
    }
}
