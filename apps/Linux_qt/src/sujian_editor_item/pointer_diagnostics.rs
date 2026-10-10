use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const MOVE_SAMPLE_INTERVAL: Duration = Duration::from_millis(34);

#[derive(Default)]
struct MoveBatch {
    first_x: Option<f64>,
    first_y: Option<f64>,
    last_x: Option<f64>,
    last_y: Option<f64>,
    raw_move_count: u64,
    last_sample_at: Option<Instant>,
}

impl MoveBatch {
    fn record(
        &mut self,
        event: &str,
        source: &str,
        sequence: Option<u64>,
        x: f64,
        y: f64,
        buttons: i32,
        pressed: bool,
        selecting: bool,
        mut extra: BTreeMap<String, Value>,
    ) {
        if self.first_x.is_none() {
            self.first_x = Some(x);
            self.first_y = Some(y);
        }
        self.last_x = Some(x);
        self.last_y = Some(y);
        self.raw_move_count = self.raw_move_count.saturating_add(1);

        let now = Instant::now();
        let should_sample = self.last_sample_at.map_or(true, |last| {
            now.saturating_duration_since(last) >= MOVE_SAMPLE_INTERVAL
        });
        if !should_sample {
            return;
        }
        self.last_sample_at = Some(now);
        extra.insert("source".to_string(), json!(source));
        extra.insert("x".to_string(), json!(x));
        extra.insert("y".to_string(), json!(y));
        extra.insert("buttons".to_string(), json!(buttons));
        extra.insert("pressed".to_string(), json!(pressed));
        extra.insert("selecting".to_string(), json!(selecting));
        extra.insert("raw_move_count".to_string(), json!(self.raw_move_count));
        extra.insert("first_x".to_string(), json!(self.first_x));
        extra.insert("first_y".to_string(), json!(self.first_y));
        record_event(event, sequence, extra);
    }

    fn flush(&mut self, source: &str, sequence: Option<u64>) {
        if self.raw_move_count == 0 {
            return;
        }
        let mut fields = BTreeMap::new();
        fields.insert("source".to_string(), json!(source));
        fields.insert("raw_move_count".to_string(), json!(self.raw_move_count));
        fields.insert("first_x".to_string(), json!(self.first_x));
        fields.insert("first_y".to_string(), json!(self.first_y));
        fields.insert("last_x".to_string(), json!(self.last_x));
        fields.insert("last_y".to_string(), json!(self.last_y));
        record_event("editor.pointer.move_summary", sequence, fields);
        *self = Self::default();
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PendingPointerRender {
    pub sequence: u64,
    pub first_frame_logged: bool,
}

#[derive(Default)]
pub(crate) struct PointerDiagnostics {
    sequence_counter: u64,
    active_sequence: Option<u64>,
    window_moves: MoveBatch,
    qml_moves: MoveBatch,
    pending_render: Option<PendingPointerRender>,
}

impl PointerDiagnostics {
    fn begin_sequence(&mut self) -> u64 {
        self.sequence_counter = self.sequence_counter.wrapping_add(1).max(1);
        self.active_sequence = Some(self.sequence_counter);
        self.sequence_counter
    }

    pub(crate) fn ensure_active_sequence(&mut self) -> u64 {
        match self.active_sequence {
            Some(sequence) => sequence,
            None => self.begin_sequence(),
        }
    }

    pub(crate) fn active_sequence(&self) -> Option<u64> {
        self.active_sequence
    }

    pub(crate) fn close_sequence(&mut self) {
        let sequence = self.active_sequence;
        self.flush_moves(sequence);
        self.active_sequence = None;
    }

    pub(crate) fn flush_moves(&mut self, sequence: Option<u64>) {
        self.window_moves.flush("window", sequence);
        self.qml_moves.flush("qml", sequence);
    }

    pub(crate) fn record_qml_activity(
        &mut self,
        kind: &str,
        x: f32,
        y: f32,
        buttons: i32,
        selecting: bool,
    ) {
        let event = match kind {
            "enter" => "editor.pointer.enter",
            "leave" => "editor.pointer.leave",
            "move" => "editor.pointer.move",
            "press" => "editor.pointer.press",
            "release" => "editor.pointer.release",
            "cancel" => "editor.pointer.cancel",
            "long_press" => "editor.pointer.long_press",
            "context_menu" => "editor.pointer.context_menu",
            _ => return,
        };
        if kind == "press" {
            if self.active_sequence.is_none() {
                self.begin_sequence();
            }
        }
        let sequence = self.active_sequence;
        if kind == "move" {
            self.qml_moves.record(
                event,
                "qml",
                sequence,
                f64::from(x),
                f64::from(y),
                buttons,
                buttons != 0,
                selecting,
                BTreeMap::new(),
            );
            return;
        }

        let mut fields = BTreeMap::new();
        fields.insert("x".to_string(), json!(x));
        fields.insert("y".to_string(), json!(y));
        fields.insert("buttons".to_string(), json!(buttons));
        let pressed = match kind {
            "press" | "long_press" => true,
            "move" | "enter" | "leave" => buttons != 0,
            _ => false,
        };
        fields.insert("pressed".to_string(), json!(pressed));
        fields.insert("selecting".to_string(), json!(selecting));
        record_event(event, sequence, fields);

        if kind == "leave" {
            self.window_moves.flush("window", sequence);
        }
        if kind == "leave" || kind == "release" || kind == "cancel" {
            self.qml_moves.flush("qml", sequence);
        }
        if kind == "context_menu" {
            self.close_sequence();
        }
    }

    pub(crate) fn record_window_activity(
        &mut self,
        kind: i32,
        window_x: f64,
        window_y: f64,
        editor_x: f64,
        editor_y: f64,
        button: i32,
        buttons: i32,
        modifiers: i32,
        inside_editor: bool,
        active_focus: bool,
        qt_timestamp: u64,
        wheel_pixel_x: i32,
        wheel_pixel_y: i32,
        wheel_angle_x: i32,
        wheel_angle_y: i32,
    ) {
        if kind == 2 || kind == 5 {
            self.window_moves.flush("window", self.active_sequence);
            self.qml_moves.flush("qml", self.active_sequence);
            self.begin_sequence();
        }
        let sequence = self.active_sequence;
        let event = match kind {
            1 => "editor.pointer.window_move",
            2 => "editor.pointer.window_press",
            3 => "editor.pointer.window_release",
            4 => "editor.pointer.window_wheel",
            5 => "editor.pointer.window_double_click",
            _ => return,
        };
        if kind == 1 {
            let mut fields = base_window_fields(
                event,
                window_x,
                window_y,
                editor_x,
                editor_y,
                button,
                buttons,
                modifiers,
                inside_editor,
                active_focus,
                qt_timestamp,
            );
            fields.insert("wheel_pixel_x".to_string(), json!(wheel_pixel_x));
            fields.insert("wheel_pixel_y".to_string(), json!(wheel_pixel_y));
            fields.insert("wheel_angle_x".to_string(), json!(wheel_angle_x));
            fields.insert("wheel_angle_y".to_string(), json!(wheel_angle_y));
            self.window_moves.record(
                event,
                "window",
                sequence,
                editor_x,
                editor_y,
                buttons,
                buttons != 0,
                false,
                fields,
            );
            return;
        }

        let mut fields = base_window_fields(
            event,
            window_x,
            window_y,
            editor_x,
            editor_y,
            button,
            buttons,
            modifiers,
            inside_editor,
            active_focus,
            qt_timestamp,
        );
        fields.insert("wheel_pixel_x".to_string(), json!(wheel_pixel_x));
        fields.insert("wheel_pixel_y".to_string(), json!(wheel_pixel_y));
        fields.insert("wheel_angle_x".to_string(), json!(wheel_angle_x));
        fields.insert("wheel_angle_y".to_string(), json!(wheel_angle_y));
        record_event(event, sequence, fields);
        if kind == 3 || kind == 4 {
            self.window_moves.flush("window", sequence);
        }
    }

    pub(crate) fn pending_render(&self) -> Option<PendingPointerRender> {
        self.pending_render
    }

    pub(crate) fn set_pending_render(&mut self, sequence: u64) {
        self.pending_render = Some(PendingPointerRender {
            sequence,
            first_frame_logged: false,
        });
    }

    pub(crate) fn mark_pending_render_frame_logged(&mut self) {
        if let Some(pending) = &mut self.pending_render {
            pending.first_frame_logged = true;
        }
    }

    pub(crate) fn clear_pending_render(&mut self) {
        self.pending_render = None;
    }
}

fn base_window_fields(
    event_type: &str,
    window_x: f64,
    window_y: f64,
    editor_x: f64,
    editor_y: f64,
    button: i32,
    buttons: i32,
    modifiers: i32,
    inside_editor: bool,
    active_focus: bool,
    qt_timestamp: u64,
) -> BTreeMap<String, Value> {
    let mut fields = BTreeMap::new();
    fields.insert("event_type".to_string(), json!(event_type));
    fields.insert("window_x".to_string(), json!(window_x));
    fields.insert("window_y".to_string(), json!(window_y));
    fields.insert("editor_x".to_string(), json!(editor_x));
    fields.insert("editor_y".to_string(), json!(editor_y));
    fields.insert("button".to_string(), json!(button));
    fields.insert("buttons".to_string(), json!(buttons));
    fields.insert("modifiers".to_string(), json!(modifiers));
    fields.insert("pressed".to_string(), json!(buttons != 0));
    fields.insert("inside_editor".to_string(), json!(inside_editor));
    fields.insert("active_focus".to_string(), json!(active_focus));
    fields.insert("qt_timestamp".to_string(), json!(qt_timestamp));
    fields
}

pub(crate) fn record_event(
    event: &str,
    sequence: Option<u64>,
    mut fields: BTreeMap<String, Value>,
) {
    fields.insert("pointer_sequence".to_string(), json!(sequence));
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level: writer_diagnostics::DiagnosticLevel::Info,
        origin: writer_diagnostics::DiagnosticOrigin::App,
        event: event.to_string(),
        target: "editor.pointer".to_string(),
        message: None,
        fields,
    });
}
