pub mod aggregate;
pub mod api;
pub mod store;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    HumanTyped,
    Pasted,
    Deleted,
    AiInserted,
    SyncRemote,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Linux,
    Android,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Linux => write!(f, "linux"),
            Platform::Android => write!(f, "android"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingInputEvent {
    pub event_id: String,
    pub timestamp_ms: i64,
    pub device_id: String,
    pub platform: Platform,
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub source: EventSource,
    pub inserted_chars: u32,
    pub deleted_chars: u32,
    pub pasted_chars: u32,
    pub ai_inserted_chars: u32,
    pub net_delta_chars: i32,
    pub session_id: String,
}

impl WritingInputEvent {
    pub fn new(
        device_id: &str,
        platform: Platform,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        source: EventSource,
        inserted_chars: u32,
        deleted_chars: u32,
        pasted_chars: u32,
        ai_inserted_chars: u32,
        session_id: &str,
    ) -> Self {
        let net = inserted_chars as i32 + pasted_chars as i32 + ai_inserted_chars as i32
            - deleted_chars as i32;
        Self {
            event_id: Uuid::new_v4().to_string(),
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            device_id: device_id.to_string(),
            platform,
            project_id: project_id.to_string(),
            volume_id: volume_id.to_string(),
            chapter_id: chapter_id.to_string(),
            source,
            inserted_chars,
            deleted_chars,
            pasted_chars,
            ai_inserted_chars,
            net_delta_chars: net,
            session_id: session_id.to_string(),
        }
    }

    pub fn human_typed_chars(&self) -> u32 {
        self.inserted_chars
    }

    pub fn is_input_event(&self) -> bool {
        matches!(self.source, EventSource::HumanTyped | EventSource::Pasted)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DateRange {
    pub start_date: String,
    pub end_date: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_creation() {
        let event = WritingInputEvent::new(
            "device-1",
            Platform::Linux,
            "proj1",
            "vol1",
            "chap1",
            EventSource::HumanTyped,
            10,
            0,
            0,
            0,
            "session-1",
        );
        assert_eq!(event.inserted_chars, 10);
        assert_eq!(event.deleted_chars, 0);
        assert_eq!(event.net_delta_chars, 10);
        assert_eq!(event.source, EventSource::HumanTyped);
        assert!(event.is_input_event());
    }

    #[test]
    fn test_event_with_deletion() {
        let event = WritingInputEvent::new(
            "device-1",
            Platform::Linux,
            "proj1",
            "vol1",
            "chap1",
            EventSource::Deleted,
            0,
            5,
            0,
            0,
            "session-1",
        );
        assert_eq!(event.net_delta_chars, -5);
        assert!(!event.is_input_event());
    }

    #[test]
    fn test_event_with_paste() {
        let event = WritingInputEvent::new(
            "device-1",
            Platform::Android,
            "proj1",
            "vol1",
            "chap1",
            EventSource::Pasted,
            0,
            0,
            20,
            0,
            "session-1",
        );
        assert_eq!(event.pasted_chars, 20);
        assert_eq!(event.net_delta_chars, 20);
        assert!(event.is_input_event());
    }

    #[test]
    fn test_event_with_ai_insert() {
        let event = WritingInputEvent::new(
            "device-1",
            Platform::Linux,
            "proj1",
            "vol1",
            "chap1",
            EventSource::AiInserted,
            0,
            0,
            0,
            50,
            "session-1",
        );
        assert_eq!(event.ai_inserted_chars, 50);
        assert_eq!(event.net_delta_chars, 50);
        assert!(!event.is_input_event());
    }

    #[test]
    fn test_event_sync_remote() {
        let event = WritingInputEvent::new(
            "device-1",
            Platform::Linux,
            "proj1",
            "vol1",
            "chap1",
            EventSource::SyncRemote,
            0,
            0,
            0,
            0,
            "session-1",
        );
        assert!(!event.is_input_event());
    }
}
