use cpp::cpp;
use qmetaobject::QString;

use crate::sujian_editor_item::sujian_editor_debug_enabled;

cpp! {{
    #include <QtGui/QFont>
    #include <QtGui/QFontMetricsF>
    #include <QtGui/QTextLayout>
    #include <QtGui/QTextOption>
    #include <QGuiApplication>
    #include <vector>

    struct EditorLayoutEntry {
        int qcharStart;
        int qcharEnd;
        double width;
        double xPos;
        double xEndLeading;
        double xEndTrailing;
        double naturalTextWidth;
        double ascent;
        double descent;
    };
    thread_local std::vector<EditorLayoutEntry> g_editor_layout_buf;

    double editor_layout_cursor_to_x(
        const QString& paraText, double fs, const QString& ff,
        const QString& textBeforeCursor
    ) {
        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QTextLayout layout(paraText, font);
        QTextOption option;
        option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
        layout.setTextOption(option);
        layout.beginLayout();

        int qchar_count = textBeforeCursor.size();
        double x = 0.0;
        while (true) {
            QTextLine line = layout.createLine();
            if (!line.isValid()) break;
            int line_start = line.textStart();
            int line_end = line_start + line.textLength();
            if (qchar_count >= line_start && qchar_count <= line_end) {
                int pos = qchar_count;
                if (pos < line_start) pos = line_start;
                if (pos > line_end) pos = line_end;
                x = line.cursorToX(pos);
                break;
            }
        }
        layout.endLayout();
        return x;
    }

    double editor_layout_cursor_to_x_on_line(
        const QString& paraText, int cursor_qchar,
        double fs, const QString& ff,
        double paragraph_wrap_w, double indent_w, int qtextline_idx, bool use_trailing
    ) {
        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QTextLayout layout(paraText, font);
        QTextOption option;
        option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
        layout.setTextOption(option);
        layout.beginLayout();

        double x = 0.0;
        int cur_idx = 0;
        bool first = true;
        while (true) {
            QTextLine line = layout.createLine();
            if (!line.isValid()) break;
            double lineWrap = first ? (paragraph_wrap_w - indent_w) : paragraph_wrap_w;
            line.setLineWidth(lineWrap);
            if (cur_idx == qtextline_idx) {
                int line_start = line.textStart();
                int line_end = line_start + line.textLength();
                int pos = cursor_qchar;
                if (pos < line_start) pos = line_start;
                if (pos > line_end) pos = line_end;
                x = line.cursorToX(pos, use_trailing ? QTextLine::Trailing : QTextLine::Leading);
                if (qEnvironmentVariableIsSet("SUJIAN_EDITOR_DEBUG")) {
                    qDebug("[cursor_to_x] qtextline=%d line_start=%d line_end=%d cursor_qchar=%d pos=%d x=%.4f trailing=%d naturalW=%.4f",
                        qtextline_idx, line_start, line_end, cursor_qchar, pos, x, (int)use_trailing, line.naturalTextWidth());
                }
                break;
            }
            first = false;
            cur_idx++;
        }
        layout.endLayout();
        return x;
    }

    int editor_layout_x_to_cursor_on_line(
        const QString& paraText, double x,
        double fs, const QString& ff,
        double paragraph_wrap_w, double indent_w, int qtextline_idx
    ) {
        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QTextLayout layout(paraText, font);
        QTextOption option;
        option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
        layout.setTextOption(option);
        layout.beginLayout();

        int target_idx = 0;
        int cur_idx = 0;
        bool first = true;
        while (true) {
            QTextLine line = layout.createLine();
            if (!line.isValid()) break;
            double lineWrap = first ? (paragraph_wrap_w - indent_w) : paragraph_wrap_w;
            line.setLineWidth(lineWrap);
            if (cur_idx == qtextline_idx) {
                int line_start = line.textStart();
                int line_end = line_start + line.textLength();
                int pos = line.xToCursor(x);
                if (pos < line_start) pos = line_start;
                if (pos > line_end) pos = line_end;
                target_idx = pos;
                if (qEnvironmentVariableIsSet("SUJIAN_EDITOR_DEBUG")) {
                    qDebug("[x_to_cursor] qtextline=%d line_start=%d line_end=%d input_x=%.4f raw_xToCursor=%d clamped_pos=%d naturalW=%.4f",
                        qtextline_idx, line_start, line_end, x, line.xToCursor(x), pos, line.naturalTextWidth());
                }
                break;
            }
            first = false;
            cur_idx++;
        }
        layout.endLayout();
        return target_idx;
    }

    void editor_layout_lines(
        const QString& text_qstr, double fs, const QString& ff,
        double wrap_w, double indent_w
    ) {
        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QTextLayout layout(text_qstr, font);
        QTextOption option;
        option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
        layout.setTextOption(option);
        layout.beginLayout();

        g_editor_layout_buf.clear();
        bool first = true;
        while (true) {
            QTextLine line = layout.createLine();
            if (!line.isValid()) break;
            double lineWrap = first ? (wrap_w - indent_w) : wrap_w;
            line.setLineWidth(lineWrap);
            EditorLayoutEntry e;
            e.qcharStart = line.textStart();
            e.qcharEnd = line.textStart() + line.textLength();
            e.width = line.naturalTextWidth();
            e.xPos = first ? indent_w : 0.0;
            e.xEndLeading = line.cursorToX(e.qcharEnd, QTextLine::Leading);
            e.xEndTrailing = line.cursorToX(e.qcharEnd, QTextLine::Trailing);
            e.naturalTextWidth = line.naturalTextWidth();
            e.ascent = line.ascent();
            e.descent = line.descent();
            g_editor_layout_buf.push_back(e);
            first = false;
        }
        layout.endLayout();
    }

    int editor_layout_entry_count() {
        return static_cast<int>(g_editor_layout_buf.size());
    }

    void editor_layout_debug_line_metrics(
        const QString& paraText, double fs, const QString& ff,
        double paragraph_wrap_w, double indent_w, int qtextline_idx
    ) {
        if (!qEnvironmentVariableIsSet("SUJIAN_EDITOR_DEBUG")) return;
        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QTextLayout layout(paraText, font);
        QTextOption option;
        option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
        layout.setTextOption(option);
        layout.beginLayout();

        int cur_idx = 0;
        bool first = true;
        while (true) {
            QTextLine line = layout.createLine();
            if (!line.isValid()) break;
            double lineWrap = first ? (paragraph_wrap_w - indent_w) : paragraph_wrap_w;
            line.setLineWidth(lineWrap);
            if (cur_idx == qtextline_idx) {
                int line_start = line.textStart();
                int line_end = line_start + line.textLength();
                qDebug("[debug_line_metrics] qtextline=%d exists=1 textStart=%d textLength=%d lineEnd=%d width=%.4f naturalTextWidth=%.4f cursorToX(textStart,Leading)=%.4f cursorToX(lineEnd,Leading)=%.4f cursorToX(lineEnd,Trailing)=%.4f xToCursor(naturalTextWidth)=%d xToCursor(width)=%d",
                    qtextline_idx, line_start, line.textLength(), line_end,
                    line.width(), line.naturalTextWidth(),
                    line.cursorToX(line_start, QTextLine::Leading),
                    line.cursorToX(line_end, QTextLine::Leading),
                    line.cursorToX(line_end, QTextLine::Trailing),
                    line.xToCursor(line.naturalTextWidth()),
                    line.xToCursor(line.width()));
                break;
            }
            first = false;
            cur_idx++;
        }
        layout.endLayout();
    }

    void editor_layout_glyph_positions_on_line(
        const QString& paraText, int range_qchar_start, int range_qchar_end,
        double fs, const QString& ff, double paragraph_wrap_w, double indent_w, int qtextline_idx
    ) {
        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QTextLayout layout(paraText, font);
        QTextOption option;
        option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
        layout.setTextOption(option);
        layout.beginLayout();

        g_editor_layout_buf.clear();
        int cur_idx = 0;
        bool first = true;
        while (true) {
            QTextLine line = layout.createLine();
            if (!line.isValid()) break;
            double lineWrap = first ? (paragraph_wrap_w - indent_w) : paragraph_wrap_w;
            line.setLineWidth(lineWrap);
            if (cur_idx == qtextline_idx) {
                int line_start = line.textStart();
                int line_end = line_start + line.textLength();
                int r_start = std::max(range_qchar_start, line_start);
                int r_end = std::min(range_qchar_end, line_end);
                for (int i = r_start; i < r_end; i++) {
                    double x1 = line.cursorToX(i);
                    double x2 = line.cursorToX(i + 1);
                    EditorLayoutEntry e;
                    e.qcharStart = i;
                    e.width = std::abs(x2 - x1);
                    e.xPos = x1;
                    g_editor_layout_buf.push_back(e);
                }
                break;
            }
            first = false;
            cur_idx++;
        }
        layout.endLayout();
    }
}}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaretAffinity {
    Upstream,
    Downstream,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VisualLine {
    pub id: usize,
    pub start: usize,
    pub end: usize,
    pub hard_break: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub para_text: String,
    pub para_start: usize,
    pub qtextline_idx: i32,
    pub para_qchar_start: usize,
    pub para_qchar_end: usize,
    pub line_wrap_width: f64,
    pub line_indent_x: f64,
    pub para_indent: f64,
    pub x_end_trailing: f64,
    pub qt_ascent: f64,
    pub qt_descent: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaretRect {
    pub x: f64,
    pub y: f64,
    pub h: f64,
    pub visual_line_id: usize,
    pub visible: bool,
}

pub type CursorLayoutRect = CaretRect;

#[derive(Clone, Debug)]
pub struct LayoutParams {
    pub width: f64,
    pub font_size: f32,
    pub font_family: String,
    pub line_spacing: f32,
    pub text_indent: f32,
    pub padding: f32,
}

#[derive(Clone, Debug)]
pub struct LayoutSnapshot {
    pub text_ptr: usize,
    pub text_len: usize,
    pub width: f64,
    pub font_size: f32,
    pub font_family: String,
    pub line_spacing: f32,
    pub text_indent: f32,
    pub padding: f32,
    pub lines: Vec<VisualLine>,
    pub content_height: f32,
}

pub type LayoutCache = LayoutSnapshot;

#[derive(Default)]
pub struct EditorLayout {
    cache: Option<LayoutSnapshot>,
}

impl EditorLayout {
    pub fn invalidate(&mut self) {
        self.cache = None;
    }

    pub fn cache(&self) -> Option<&LayoutSnapshot> {
        self.cache.as_ref()
    }

    pub fn snapshot(&mut self, text: &str, params: LayoutParams) -> &LayoutSnapshot {
        let text_ptr = text.as_ptr() as usize;
        let text_len = text.len();
        let needs_refresh = match &self.cache {
            Some(c) => {
                c.text_ptr != text_ptr
                    || c.text_len != text_len
                    || (c.width - params.width).abs() > 0.1
                    || (c.font_size - params.font_size).abs() > 0.1
                    || c.font_family != params.font_family
                    || (c.line_spacing - params.line_spacing).abs() > 0.01
                    || (c.text_indent - params.text_indent).abs() > 0.1
                    || (c.padding - params.padding).abs() > 0.1
            }
            None => true,
        };

        if needs_refresh {
            let lines = layout_lines(
                text,
                params.width,
                params.font_size as f64,
                params.line_spacing as f64,
                params.padding as f64,
                params.text_indent as f64,
                &params.font_family,
            );
            let content_height = lines
                .last()
                .map(|l| (l.y + l.height + params.padding as f64) as f32)
                .unwrap_or((params.font_size * params.line_spacing + params.padding * 2.0) as f32)
                .max(1.0);
            self.cache = Some(LayoutSnapshot {
                text_ptr,
                text_len,
                width: params.width,
                font_size: params.font_size,
                font_family: params.font_family,
                line_spacing: params.line_spacing,
                text_indent: params.text_indent,
                padding: params.padding,
                lines,
                content_height,
            });
        }

        self.cache.as_ref().unwrap()
    }

    #[cfg(test)]
    pub fn replace_snapshot(&mut self, snapshot: LayoutSnapshot) {
        self.cache = Some(snapshot);
    }

    pub fn hit_test(
        &self,
        snapshot: &LayoutSnapshot,
        x: f64,
        y: f64,
        scroll_y: f64,
    ) -> (usize, CaretAffinity) {
        hit_test(snapshot, x, y, scroll_y)
    }

    pub fn caret_rect(
        &self,
        snapshot: &LayoutSnapshot,
        cursor_byte: usize,
        affinity: CaretAffinity,
        scroll_y: f64,
        viewport_h: f64,
    ) -> CaretRect {
        caret_rect(snapshot, cursor_byte, affinity, scroll_y, viewport_h)
    }

    pub fn cursor_geometry(
        &self,
        snapshot: &LayoutSnapshot,
        cursor: usize,
        affinity: CaretAffinity,
    ) -> (f64, f64, usize) {
        cursor_geometry(snapshot, cursor, affinity)
    }

    pub fn cursor_line_and_x(
        &self,
        snapshot: &LayoutSnapshot,
        cursor: usize,
        affinity: CaretAffinity,
    ) -> Option<(usize, f64)> {
        cursor_line_and_x(snapshot, cursor, affinity)
    }

    pub fn index_at_line_x(&self, snapshot: &LayoutSnapshot, line: &VisualLine, x: f64) -> usize {
        index_at_line_x(snapshot, line, x)
    }

    pub fn cursor_x_for_line(
        &self,
        snapshot: &LayoutSnapshot,
        line: &VisualLine,
        cursor: usize,
        affinity: CaretAffinity,
    ) -> f64 {
        calculate_cursor_x_for_line(line, cursor, affinity, snapshot)
    }

    pub fn glyph_positions_on_line(
        &self,
        line: &VisualLine,
        range_start: usize,
        range_end: usize,
        font_size: f64,
        font_family: &str,
    ) -> Vec<(usize, f64, f64)> {
        qtextlayout_glyph_positions_on_line(
            &line.para_text,
            range_start,
            range_end,
            line.para_start,
            font_size,
            font_family,
            line.line_wrap_width + line.line_indent_x,
            line.para_indent,
            line.qtextline_idx,
        )
    }

    pub fn text_width(&self, text: &str, font_size: f64, font_family: &str) -> f64 {
        qtextlayout_cursor_to_x(text, text, font_size, font_family)
    }

    pub fn cursor_height(&self, font_size: f64, font_family: &str) -> f64 {
        cursor_height_for_line(font_size, font_family)
    }

    pub fn cursor_rect_for_line(
        &self,
        line: &VisualLine,
        font_size: f64,
        font_family: &str,
    ) -> (f64, f64) {
        cursor_rect_for_line(line, font_size, font_family)
    }

    pub fn text_baseline_y(&self, line: &VisualLine, font_size: f64, font_family: &str) -> f64 {
        text_baseline_y(line, font_size, font_family)
    }

    pub fn affinity_for_index_on_line(&self, line: &VisualLine, index: usize) -> CaretAffinity {
        affinity_for_index_on_line(line, index)
    }

    pub fn line_contains_cursor_with_affinity(
        &self,
        lines: &[VisualLine],
        idx: usize,
        cursor: usize,
        affinity: CaretAffinity,
    ) -> bool {
        line_contains_cursor_with_affinity(lines, idx, cursor, affinity)
    }
}

pub fn layout_lines(
    text: &str,
    width: f64,
    font_size: f64,
    line_spacing: f64,
    padding: f64,
    indent: f64,
    font_family: &str,
) -> Vec<VisualLine> {
    let line_height = (font_size * line_spacing).max(font_size + 4.0);
    let available = (width - padding * 2.0).max(font_size);
    let mut result = Vec::new();
    let mut y = padding;
    let mut paragraph_start = 0;
    let mut line_id: usize = 0;

    for paragraph in text.split_inclusive('\n') {
        let hard_break = paragraph.ends_with('\n');
        let paragraph_text = paragraph.trim_end_matches('\n');
        let paragraph_text_end = paragraph_start + paragraph_text.len();

        if paragraph_text.is_empty() {
            result.push(VisualLine {
                id: line_id,
                start: paragraph_start,
                end: paragraph_start,
                hard_break,
                x: padding + indent,
                y,
                width: 0.0,
                height: line_height,
                para_text: String::new(),
                para_start: paragraph_start,
                qtextline_idx: 0,
                para_qchar_start: 0,
                para_qchar_end: 0,
                line_wrap_width: available - indent,
                line_indent_x: indent,
                para_indent: indent,
                x_end_trailing: 0.0,
                qt_ascent: 0.0,
                qt_descent: 0.0,
            });
            line_id += 1;
            y += line_height;
            paragraph_start += paragraph.len();
            continue;
        }

        let para_start = paragraph_start;
        let fs = font_size as f32;
        let ff: QString = font_family.to_string().into();
        let wrap_w = available;
        let indent_w = indent;
        let text_qstr: QString = paragraph_text.to_string().into();

        let line_count = cpp!(unsafe [
            text_qstr as "QString",
            fs as "float",
            ff as "QString",
            wrap_w as "double",
            indent_w as "double"
        ] -> i32 as "int" {
            editor_layout_lines(text_qstr, fs, ff, wrap_w, indent_w);
            return static_cast<int>(g_editor_layout_buf.size());
        });

        for line_idx in 0..line_count {
            let idx = line_idx;
            let qchar_off = cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
                if (idx >= 0 && idx < static_cast<int>(g_editor_layout_buf.size())) {
                    return static_cast<qulonglong>(g_editor_layout_buf[idx].qcharStart);
                }
                return 0;
            });
            let qchar_end = cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
                if (idx >= 0 && idx < static_cast<int>(g_editor_layout_buf.size())) {
                    return static_cast<qulonglong>(g_editor_layout_buf[idx].qcharEnd);
                }
                return 0;
            });
            let line_w = cpp!(unsafe [idx as "int"] -> f64 as "double" {
                if (idx >= 0 && idx < static_cast<int>(g_editor_layout_buf.size())) {
                    return g_editor_layout_buf[idx].width;
                }
                return 0.0;
            });
            let x_off = cpp!(unsafe [idx as "int"] -> f64 as "double" {
                if (idx >= 0 && idx < static_cast<int>(g_editor_layout_buf.size())) {
                    return g_editor_layout_buf[idx].xPos;
                }
                return 0.0;
            });
            let x_end_trailing = cpp!(unsafe [idx as "int"] -> f64 as "double" {
                if (idx >= 0 && idx < static_cast<int>(g_editor_layout_buf.size())) {
                    return g_editor_layout_buf[idx].xEndTrailing;
                }
                return 0.0;
            });
            let qt_ascent = cpp!(unsafe [idx as "int"] -> f64 as "double" {
                if (idx >= 0 && idx < static_cast<int>(g_editor_layout_buf.size())) {
                    return g_editor_layout_buf[idx].ascent;
                }
                return 0.0;
            });
            let qt_descent = cpp!(unsafe [idx as "int"] -> f64 as "double" {
                if (idx >= 0 && idx < static_cast<int>(g_editor_layout_buf.size())) {
                    return g_editor_layout_buf[idx].descent;
                }
                return 0.0;
            });

            let qt_metrics_h = qt_ascent + qt_descent;
            let actual_line_h = if qt_metrics_h > 0.0 {
                line_height.max(qt_metrics_h)
            } else {
                line_height
            };

            let byte_off = qchar_offset_to_byte_offset(paragraph_text, qchar_off);
            let abs_start = para_start + byte_off;
            let abs_end = para_start + qchar_offset_to_byte_offset(paragraph_text, qchar_end);
            let is_first = line_idx == 0;

            result.push(VisualLine {
                id: line_id,
                start: abs_start,
                end: abs_end,
                hard_break: hard_break && abs_end == paragraph_text_end,
                x: padding + x_off,
                y,
                width: line_w,
                height: actual_line_h,
                para_text: paragraph_text.to_string(),
                para_start,
                qtextline_idx: line_idx as i32,
                para_qchar_start: qchar_off,
                para_qchar_end: qchar_end,
                line_wrap_width: if is_first {
                    available - indent
                } else {
                    available
                },
                line_indent_x: if is_first { indent } else { 0.0 },
                para_indent: indent,
                x_end_trailing: x_end_trailing,
                qt_ascent,
                qt_descent,
            });
            line_id += 1;
            y += actual_line_h;
        }

        paragraph_start += paragraph.len();
    }

    if text.ends_with('\n') {
        result.push(VisualLine {
            id: line_id,
            start: text.len(),
            end: text.len(),
            hard_break: false,
            x: padding + indent,
            y,
            width: 0.0,
            height: line_height,
            para_text: String::new(),
            para_start: text.len(),
            qtextline_idx: 0,
            para_qchar_start: 0,
            para_qchar_end: 0,
            line_wrap_width: available - indent,
            line_indent_x: indent,
            para_indent: indent,
            x_end_trailing: 0.0,
            qt_ascent: 0.0,
            qt_descent: 0.0,
        });
        line_id += 1;
    }

    if text.is_empty() {
        result.push(VisualLine {
            id: line_id,
            start: 0,
            end: 0,
            hard_break: false,
            x: padding + indent,
            y,
            width: 0.0,
            height: line_height,
            para_text: String::new(),
            para_start: 0,
            qtextline_idx: 0,
            para_qchar_start: 0,
            para_qchar_end: 0,
            line_wrap_width: available - indent,
            line_indent_x: indent,
            para_indent: indent,
            x_end_trailing: 0.0,
            qt_ascent: 0.0,
            qt_descent: 0.0,
        });
    }

    result
}

pub fn hit_test(
    snapshot: &LayoutSnapshot,
    x: f64,
    y: f64,
    scroll_y: f64,
) -> (usize, CaretAffinity) {
    let lines = &snapshot.lines;
    if lines.is_empty() {
        return (0, CaretAffinity::Downstream);
    }

    let doc_y = y + scroll_y;
    let line_opt = lines
        .iter()
        .enumerate()
        .find(|(_, line)| doc_y < line.y + line.height);
    let (_line_idx, line) = match line_opt {
        Some((idx, l)) => (idx, l),
        None => {
            let Some(last) = lines.last() else {
                return (0, CaretAffinity::Downstream);
            };
            (lines.len() - 1, last)
        }
    };
    let raw_index = index_at_line_x(snapshot, line, x);
    let index = raw_index.max(line.start).min(line.end);
    debug_assert!(
        index >= line.start && index <= line.end,
        "hit_test: index {} out of line range {}..{}",
        index,
        line.start,
        line.end
    );
    let affinity = affinity_for_index_on_line(line, index);

    if std::env::var("SUJIAN_EDITOR_DEBUG").is_ok() {
        eprintln!(
            "[hit_test] cursor={}, affinity={:?}, hit_visual_line_id={}, line.start={}, line.end={}, line.x={:.1}, line.y={:.1}, line.width={:.1}, line.para_start={}, line.qtextline_idx={}, line.para_qchar_start={}, line.para_qchar_end={}, line.line_wrap_width={:.1}, line.line_indent_x={:.1}, line.para_indent={:.1}, x_end_trailing={:.1}",
            index, affinity, line.id, line.start, line.end, line.x, line.y, line.width,
            line.para_start, line.qtextline_idx, line.para_qchar_start, line.para_qchar_end,
            line.line_wrap_width, line.line_indent_x, line.para_indent, line.x_end_trailing
        );
        debug_line_metrics(
            &line.para_text,
            snapshot.font_size as f64,
            &snapshot.font_family,
            line.line_wrap_width + line.line_indent_x,
            line.para_indent,
            line.qtextline_idx,
        );
    }

    #[cfg(debug_assertions)]
    {
        let rect = caret_rect(snapshot, index, affinity, scroll_y, f64::INFINITY);
        let rect_y_doc = rect.y + scroll_y;
        let line_top = line.y;
        let line_bottom = line.y + line.height;
        let diff = if rect_y_doc < line_top {
            line_top - rect_y_doc
        } else if rect_y_doc > line_bottom {
            rect_y_doc - line_bottom
        } else {
            0.0
        };
        debug_assert!(
            diff < 5.0,
            "hit_test debug assert failed: rect_y_doc={:.2} is not within hit line range {:.2}..{:.2} (diff={:.2})",
            rect_y_doc,
            line_top,
            line_bottom,
            diff
        );
    }

    (index, affinity)
}

pub fn caret_rect(
    snapshot: &LayoutSnapshot,
    cursor_byte: usize,
    affinity: CaretAffinity,
    scroll_y: f64,
    viewport_h: f64,
) -> CaretRect {
    let line = snapshot
        .lines
        .iter()
        .enumerate()
        .find(|(idx, _)| {
            line_contains_cursor_with_affinity(&snapshot.lines, *idx, cursor_byte, affinity)
        })
        .map(|(_, line)| line)
        .or_else(|| snapshot.lines.last());

    let fallback;
    let line = match line {
        Some(line) => line,
        None => {
            fallback = VisualLine {
                id: 0,
                start: 0,
                end: 0,
                hard_break: true,
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: snapshot.font_size as f64 * snapshot.line_spacing as f64,
                para_text: String::new(),
                para_start: 0,
                qtextline_idx: 0,
                para_qchar_start: 0,
                para_qchar_end: 0,
                line_wrap_width: 0.0,
                line_indent_x: 0.0,
                para_indent: 0.0,
                x_end_trailing: 0.0,
                qt_ascent: 0.0,
                qt_descent: 0.0,
            };
            &fallback
        }
    };

    let cursor_x = calculate_cursor_x_for_line(line, cursor_byte, affinity, snapshot);
    let (cursor_y_doc, cursor_h) =
        cursor_rect_for_line(line, snapshot.font_size as f64, &snapshot.font_family);
    let cursor_y = cursor_y_doc - scroll_y;
    let visible = cursor_y + cursor_h > 0.0 && cursor_y < viewport_h.max(1.0);

    if std::env::var("SUJIAN_EDITOR_DEBUG").is_ok() {
        let ascent = if line.qt_ascent > 0.0 {
            line.qt_ascent
        } else {
            get_font_ascent(&snapshot.font_family, snapshot.font_size)
        };
        let descent = if line.qt_descent > 0.0 {
            line.qt_descent
        } else {
            get_font_descent(&snapshot.font_family, snapshot.font_size)
        };
        let text_baseline = text_baseline_y(line, snapshot.font_size as f64, &snapshot.font_family);
        let cursor_top_to_baseline = text_baseline - cursor_y_doc;
        let cursor_bottom_to_baseline = cursor_y_doc + cursor_h - text_baseline;
        eprintln!(
            "[caret_rect] cursor={}, affinity={:?}, visual_line_id={}, line.y={:.1}, line.height={:.1}, line.x={:.1}, line.width={:.1}, target_x={:.1}, target_y={:.1}, cursor_h={:.1}, text_baseline_y={:.1}, font_ascent={:.1}, font_descent={:.1}, cursor_top_to_baseline={:.1}, cursor_bottom_to_baseline={:.1}, qt_ascent={:.1}, qt_descent={:.1}",
            cursor_byte, affinity, line.id, line.y, line.height, line.x, line.width,
            cursor_x, cursor_y_doc, cursor_h, text_baseline, ascent, descent,
            cursor_top_to_baseline, cursor_bottom_to_baseline, line.qt_ascent, line.qt_descent
        );
    }

    CaretRect {
        x: cursor_x,
        y: cursor_y,
        h: cursor_h,
        visual_line_id: line.id,
        visible,
    }
}

pub fn index_at_line_x(snapshot: &LayoutSnapshot, line: &VisualLine, x: f64) -> usize {
    let relative = (x - line.x).max(0.0);
    if line.para_text.is_empty() {
        return line.start;
    }
    let paragraph_wrap_w = line.line_wrap_width + line.line_indent_x;
    qtextlayout_x_to_cursor_on_line(
        &line.para_text,
        relative,
        line.para_start,
        snapshot.font_size as f64,
        &snapshot.font_family,
        paragraph_wrap_w,
        line.para_indent,
        line.qtextline_idx,
    )
}

pub fn cursor_line_and_x(
    snapshot: &LayoutSnapshot,
    cursor: usize,
    affinity: CaretAffinity,
) -> Option<(usize, f64)> {
    let lines = &snapshot.lines;
    if lines.is_empty() {
        return None;
    }
    for (idx, line) in lines.iter().enumerate() {
        if line_contains_cursor_with_affinity(lines, idx, cursor, affinity) {
            let cursor_x = calculate_cursor_x_for_line(line, cursor, affinity, snapshot);
            return Some((idx, cursor_x));
        }
    }
    lines.last().map(|line| {
        let cursor_x = calculate_cursor_x_for_line(line, cursor, affinity, snapshot);
        (lines.len() - 1, cursor_x)
    })
}

pub fn cursor_geometry(
    snapshot: &LayoutSnapshot,
    cursor: usize,
    affinity: CaretAffinity,
) -> (f64, f64, usize) {
    for (idx, line) in snapshot.lines.iter().enumerate() {
        if line_contains_cursor_with_affinity(&snapshot.lines, idx, cursor, affinity) {
            let cursor_x = calculate_cursor_x_for_line(line, cursor, affinity, snapshot);
            let cursor_y =
                cursor_rect_for_line(line, snapshot.font_size as f64, &snapshot.font_family).0;
            return (cursor_x, cursor_y, line.id);
        }
    }
    snapshot
        .lines
        .last()
        .map(|line| {
            let cursor_x = calculate_cursor_x_for_line(line, cursor, affinity, snapshot);
            let cursor_y =
                cursor_rect_for_line(line, snapshot.font_size as f64, &snapshot.font_family).0;
            (cursor_x, cursor_y, line.id)
        })
        .unwrap_or((0.0, 0.0, 0))
}

pub fn calculate_cursor_x_for_line(
    line: &VisualLine,
    cursor: usize,
    affinity: CaretAffinity,
    snapshot: &LayoutSnapshot,
) -> f64 {
    if line.para_text.is_empty() {
        if line.width > 0.0 && cursor == line.end {
            line.x + line.width
        } else {
            line.x
        }
    } else {
        if affinity == CaretAffinity::Upstream && cursor == line.end && line.x_end_trailing > 0.0 {
            let x = line.x + line.x_end_trailing;
            if sujian_editor_debug_enabled() {
                eprintln!(
                    "[calculate_cursor_x] using cached x_end_trailing: cursor={}, line.end={}, x_end_trailing={:.4}, result_x={:.4}",
                    cursor, line.end, line.x_end_trailing, x
                );
            }
            return x;
        }
        let use_trailing = affinity == CaretAffinity::Upstream && cursor == line.end;
        let paragraph_wrap_w = line.line_wrap_width + line.line_indent_x;
        let x = line.x
            + qtextlayout_cursor_to_x_on_line(
                &line.para_text,
                cursor,
                line.para_start,
                snapshot.font_size as f64,
                &snapshot.font_family,
                paragraph_wrap_w,
                line.para_indent,
                line.qtextline_idx,
                use_trailing,
            );

        if x <= 1.0 && line.start != line.end && std::env::var("SUJIAN_EDITOR_DEBUG").is_ok() {
            let cursor_in_para = cursor.saturating_sub(line.para_start);
            let cursor_qchar = byte_offset_to_qchar_offset(&line.para_text, cursor_in_para);
            let line_end_byte_in_para = line.end.saturating_sub(line.para_start);
            let line_end_qchar =
                byte_offset_to_qchar_offset(&line.para_text, line_end_byte_in_para);
            eprintln!(
                "[INVARIANT] cursor_x <= 1.0 for non-empty line!\n\
                 VisualLine: para_qchar_start={}, para_qchar_end={}, start={}, end={}\n\
                 Qt helper: textStart=para_qchar_start ({}), lineEnd={} (from qcharEnd)\n\
                 input cursor_qchar={}, cursor_abs_byte={}, qtextline_idx={}\n\
                 line.end byte -> qchar offset={}\n\
                 Qt cursorToX result={:.4}, line.x={:.4}",
                line.para_qchar_start,
                line.para_qchar_end,
                line.start,
                line.end,
                line.para_qchar_start,
                line_end_qchar,
                cursor_qchar,
                cursor,
                line.qtextline_idx,
                line_end_qchar,
                x,
                line.x
            );
            debug_line_metrics(
                &line.para_text,
                snapshot.font_size as f64,
                &snapshot.font_family,
                paragraph_wrap_w,
                line.para_indent,
                line.qtextline_idx,
            );
        }

        x
    }
}

pub fn qtextlayout_cursor_to_x(
    para_text: &str,
    text_before_cursor: &str,
    font_size: f64,
    font_family: &str,
) -> f64 {
    let para: QString = para_text.to_string().into();
    let before: QString = text_before_cursor.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    cpp!(unsafe [para as "QString", before as "QString", fs as "float", ff as "QString"] -> f64 as "double" {
        return editor_layout_cursor_to_x(para, fs, ff, before);
    })
}

pub fn qtextlayout_cursor_to_x_on_line(
    para_text: &str,
    cursor_abs_byte: usize,
    para_start: usize,
    font_size: f64,
    font_family: &str,
    paragraph_wrap_w: f64,
    indent_w: f64,
    qtextline_idx: i32,
    use_trailing: bool,
) -> f64 {
    let cursor_in_para = cursor_abs_byte.saturating_sub(para_start);
    let cursor_qchar = byte_offset_to_qchar_offset(para_text, cursor_in_para) as i32;
    let para: QString = para_text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    cpp!(unsafe [
        para as "QString",
        cursor_qchar as "int",
        fs as "float",
        ff as "QString",
        paragraph_wrap_w as "double",
        indent_w as "double",
        qtextline_idx as "int",
        use_trailing as "bool"
    ] -> f64 as "double" {
        return editor_layout_cursor_to_x_on_line(
            para, cursor_qchar, fs, ff, paragraph_wrap_w, indent_w, qtextline_idx, use_trailing
        );
    })
}

pub fn debug_line_metrics(
    para_text: &str,
    font_size: f64,
    font_family: &str,
    paragraph_wrap_w: f64,
    indent_w: f64,
    qtextline_idx: i32,
) {
    let para: QString = para_text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    cpp!(unsafe [
        para as "QString",
        fs as "float",
        ff as "QString",
        paragraph_wrap_w as "double",
        indent_w as "double",
        qtextline_idx as "int"
    ] -> () as "void" {
        editor_layout_debug_line_metrics(para, fs, ff, paragraph_wrap_w, indent_w, qtextline_idx);
    });
}

pub fn qtextlayout_x_to_cursor_on_line(
    para_text: &str,
    x: f64,
    para_start: usize,
    font_size: f64,
    font_family: &str,
    paragraph_wrap_w: f64,
    indent_w: f64,
    qtextline_idx: i32,
) -> usize {
    let para: QString = para_text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    let qchar_off = cpp!(unsafe [
        para as "QString",
        x as "double",
        fs as "float",
        ff as "QString",
        paragraph_wrap_w as "double",
        indent_w as "double",
        qtextline_idx as "int"
    ] -> i32 as "int" {
        return editor_layout_x_to_cursor_on_line(para, x, fs, ff, paragraph_wrap_w, indent_w, qtextline_idx);
    });
    let para_byte = qchar_offset_to_byte_offset(para_text, qchar_off as usize);
    para_start + para_byte
}

pub fn qtextlayout_glyph_positions_on_line(
    para_text: &str,
    range_start: usize,
    range_end: usize,
    para_start: usize,
    font_size: f64,
    font_family: &str,
    paragraph_wrap_w: f64,
    indent_w: f64,
    qtextline_idx: i32,
) -> Vec<(usize, f64, f64)> {
    let seg_start_in_para = range_start.saturating_sub(para_start);
    let seg_end_in_para = range_end.saturating_sub(para_start).min(para_text.len());
    let qchar_start = byte_offset_to_qchar_offset(para_text, seg_start_in_para) as i32;
    let qchar_end = byte_offset_to_qchar_offset(para_text, seg_end_in_para) as i32;
    let para: QString = para_text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    let count = cpp!(unsafe [
        para as "QString",
        qchar_start as "int",
        qchar_end as "int",
        fs as "float",
        ff as "QString",
        paragraph_wrap_w as "double",
        indent_w as "double",
        qtextline_idx as "int"
    ] -> i32 as "int" {
        editor_layout_glyph_positions_on_line(
            para, qchar_start, qchar_end, fs, ff, paragraph_wrap_w, indent_w, qtextline_idx
        );
        return static_cast<int>(g_editor_layout_buf.size());
    });
    let mut result = Vec::with_capacity(count as usize);
    for i in 0..count {
        let idx = i;
        let qchar_off = cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
            return static_cast<qulonglong>(g_editor_layout_buf[idx].qcharStart);
        });
        let w = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_editor_layout_buf[idx].width;
        });
        let x_pos = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_editor_layout_buf[idx].xPos;
        });
        let para_byte = qchar_offset_to_byte_offset(para_text, qchar_off);
        let abs_byte = para_start + para_byte;
        result.push((abs_byte, x_pos, w));
    }
    result
}

pub fn byte_offset_to_qchar_offset(text: &str, byte_offset: usize) -> usize {
    text[..byte_offset.min(text.len())]
        .chars()
        .map(|c| c.len_utf16())
        .sum()
}

pub fn qchar_offset_to_byte_offset(text: &str, qchar_offset: usize) -> usize {
    let mut qchar_count: usize = 0;
    for (byte_pos, ch) in text.char_indices() {
        if qchar_count >= qchar_offset {
            return byte_pos;
        }
        qchar_count += ch.len_utf16();
    }
    text.len()
}

#[cfg(not(test))]
pub fn get_font_ascent(font_family: &str, font_size: f32) -> f64 {
    let family = QString::from(font_family);
    cpp!(unsafe [family as "QString", font_size as "float"] -> f64 as "double" {
        QFont font(family);
        font.setPixelSize(font_size);
        QFontMetricsF metrics(font);
        return metrics.ascent();
    })
}

#[cfg(test)]
pub fn get_font_ascent(_font_family: &str, font_size: f32) -> f64 {
    font_size as f64 * 0.8
}

#[cfg(not(test))]
pub fn get_font_descent(font_family: &str, font_size: f32) -> f64 {
    let family = QString::from(font_family);
    cpp!(unsafe [family as "QString", font_size as "float"] -> f64 as "double" {
        QFont font(family);
        font.setPixelSize(font_size);
        QFontMetricsF metrics(font);
        return metrics.descent();
    })
}

#[cfg(test)]
pub fn get_font_descent(_font_family: &str, font_size: f32) -> f64 {
    font_size as f64 * 0.2
}

pub fn cursor_height_for_line(font_size: f64, font_family: &str) -> f64 {
    let ascent = get_font_ascent(font_family, font_size as f32);
    let descent = get_font_descent(font_family, font_size as f32);
    ascent + descent
}

pub fn cursor_rect_for_line(line: &VisualLine, font_size: f64, font_family: &str) -> (f64, f64) {
    let ascent = if line.qt_ascent > 0.0 {
        line.qt_ascent
    } else {
        get_font_ascent(font_family, font_size as f32)
    };
    let descent = if line.qt_descent > 0.0 {
        line.qt_descent
    } else {
        get_font_descent(font_family, font_size as f32)
    };
    let baseline = text_baseline_y(line, font_size, font_family);
    let h = ascent + descent;
    let mut top_y = baseline - ascent;
    if top_y < line.y {
        top_y = line.y;
    }
    if top_y + h > line.y + line.height {
        top_y = line.y + line.height - h;
    }
    (top_y, h)
}

pub fn cursor_top_y(line: &VisualLine, font_size: f64, font_family: &str) -> f64 {
    cursor_rect_for_line(line, font_size, font_family).0
}

pub fn text_baseline_y(line: &VisualLine, font_size: f64, font_family: &str) -> f64 {
    let ascent = if line.qt_ascent > 0.0 {
        line.qt_ascent
    } else {
        get_font_ascent(font_family, font_size as f32)
    };
    let descent = if line.qt_descent > 0.0 {
        line.qt_descent
    } else {
        get_font_descent(font_family, font_size as f32)
    };
    let top_padding = (line.height - (ascent + descent)).max(0.0) / 2.0;
    line.y + top_padding + ascent
}

pub fn affinity_for_index_on_line(line: &VisualLine, index: usize) -> CaretAffinity {
    if index == line.end && line.start != line.end {
        CaretAffinity::Upstream
    } else {
        CaretAffinity::Downstream
    }
}

pub fn line_contains_cursor_with_affinity(
    lines: &[VisualLine],
    idx: usize,
    cursor: usize,
    affinity: CaretAffinity,
) -> bool {
    let line = &lines[idx];
    if line.start == line.end {
        return cursor == line.start;
    }
    if cursor > line.start && cursor < line.end {
        return true;
    }
    if cursor == line.start {
        let has_prev_overlap = idx > 0 && lines[idx - 1].end == line.start;
        if has_prev_overlap {
            return affinity == CaretAffinity::Downstream;
        }
        return true;
    }
    if cursor == line.end {
        let has_next_overlap = idx + 1 < lines.len() && lines[idx + 1].start == line.end;
        if has_next_overlap {
            return affinity == CaretAffinity::Upstream;
        }
        return true;
    }
    false
}

#[allow(dead_code)]
pub fn line_contains_cursor(lines: &[VisualLine], idx: usize, cursor: usize) -> bool {
    line_contains_cursor_with_affinity(lines, idx, cursor, CaretAffinity::Downstream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;

    static QT_INIT: Once = Once::new();

    fn init_qt() {
        QT_INIT.call_once(|| {
            std::env::set_var(
                "QT_QPA_PLATFORM",
                std::env::var("QT_QPA_PLATFORM").unwrap_or_else(|_| "offscreen".to_string()),
            );
            cpp!(unsafe [] {
                static int argc = 1;
                static char app_name[] = "sujian-layout-tests";
                static char* argv[] = { app_name, nullptr };
                if (!QGuiApplication::instance()) {
                    new QGuiApplication(argc, argv);
                }
            });
        });
    }

    fn params(width: f64) -> LayoutParams {
        LayoutParams {
            width,
            font_size: 16.0,
            font_family: "serif".to_string(),
            line_spacing: 1.5,
            text_indent: 32.0,
            padding: 16.0,
        }
    }

    fn snapshot_for(text: &str, width: f64) -> LayoutSnapshot {
        init_qt();
        let mut layout = EditorLayout::default();
        layout.snapshot(text, params(width)).clone()
    }

    fn assert_line_end_roundtrip(snapshot: &LayoutSnapshot) {
        for line in &snapshot.lines {
            if line.start == line.end || line.para_text.is_empty() {
                continue;
            }
            let x = qtextlayout_cursor_to_x_on_line(
                &line.para_text,
                line.end,
                line.para_start,
                snapshot.font_size as f64,
                &snapshot.font_family,
                line.line_wrap_width + line.line_indent_x,
                line.para_indent,
                line.qtextline_idx,
                true,
            );
            assert!(
                x > 0.01,
                "line-end cursor x must not collapse to 0: line={}, range={}..{}",
                line.id,
                line.start,
                line.end
            );
            let rect = caret_rect(snapshot, line.end, CaretAffinity::Upstream, 0.0, 800.0);
            assert_eq!(
                rect.visual_line_id, line.id,
                "caret_rect(line.end, Upstream) must stay on the source visual line"
            );
            assert!(
                rect.x > 0.01,
                "caret_rect(line.end, Upstream).x must not collapse to 0: line={}, range={}..{}",
                line.id,
                line.start,
                line.end
            );
            let roundtrip = qtextlayout_x_to_cursor_on_line(
                &line.para_text,
                x,
                line.para_start,
                snapshot.font_size as f64,
                &snapshot.font_family,
                line.line_wrap_width + line.line_indent_x,
                line.para_indent,
                line.qtextline_idx,
            );
            assert_eq!(
                roundtrip, line.end,
                "xToCursor(cursorToX(line.end)) must return line.end for line {}",
                line.id
            );
        }
    }

    #[test]
    fn qchar_byte_roundtrip() {
        let texts = [
            "hello world",
            "你好世界",
            "Hello 你好 World",
            "a😀b🎉c",
            "写作者：测试emoji🎉混合",
        ];
        for text in &texts {
            for (byte_pos, _ch) in text.char_indices() {
                let qchar = byte_offset_to_qchar_offset(text, byte_pos);
                let back = qchar_offset_to_byte_offset(text, qchar);
                assert_eq!(back, byte_pos);
            }
            let qchar_end = byte_offset_to_qchar_offset(text, text.len());
            assert_eq!(qchar_offset_to_byte_offset(text, qchar_end), text.len());
        }
    }

    #[test]
    fn chinese_layout_roundtrip() {
        let snapshot = snapshot_for("第一行中文，第二段也要准确。", 320.0);
        assert!(snapshot.lines.len() >= 1);
        assert_line_end_roundtrip(&snapshot);
    }

    #[test]
    fn english_layout_roundtrip() {
        let snapshot = snapshot_for("The quick brown fox writes a quiet desktop editor.", 320.0);
        assert!(snapshot.lines.len() >= 1);
        assert_line_end_roundtrip(&snapshot);
    }

    #[test]
    fn punctuation_hit_test_and_caret_rect_match() {
        let snapshot = snapshot_for("你好，world! 句号。", 360.0);
        let line = snapshot.lines.first().unwrap();
        let comma = "你好".len();
        let rect = caret_rect(&snapshot, comma, CaretAffinity::Downstream, 0.0, 800.0);
        let (hit, affinity) = hit_test(&snapshot, rect.x + 1.0, line.y + 2.0, 0.0);
        let hit_rect = caret_rect(&snapshot, hit, affinity, 0.0, 800.0);
        assert_eq!(hit_rect.visual_line_id, line.id);
    }

    #[test]
    fn wrapping_preserves_boundary_affinity() {
        let snapshot = snapshot_for("abcdefghijklmnopqrstuvwx yz", 96.0);
        assert!(snapshot.lines.len() > 1);
        let boundary = snapshot.lines[0].end;
        assert_eq!(
            caret_rect(&snapshot, boundary, CaretAffinity::Upstream, 0.0, 800.0).visual_line_id,
            snapshot.lines[0].id
        );
        assert_eq!(
            caret_rect(&snapshot, boundary, CaretAffinity::Downstream, 0.0, 800.0).visual_line_id,
            snapshot.lines[1].id
        );
        assert_line_end_roundtrip(&snapshot);
    }

    #[test]
    fn blank_and_trailing_newline_layout() {
        for text in ["", "\n", "\n\n", "正文\n"] {
            let snapshot = snapshot_for(text, 320.0);
            assert!(!snapshot.lines.is_empty());
            let rect = caret_rect(&snapshot, text.len(), CaretAffinity::Downstream, 0.0, 800.0);
            assert!(rect.h > 0.0);
        }
    }

    #[test]
    fn hit_test_and_caret_rect_use_same_snapshot_line() {
        let snapshot = snapshot_for("第一行会自动换行，第二行继续测试命中。", 128.0);
        for line in &snapshot.lines {
            let (index, affinity) = hit_test(
                &snapshot,
                line.x + line.width.max(1.0) / 2.0,
                line.y + 2.0,
                0.0,
            );
            let rect = caret_rect(&snapshot, index, affinity, 0.0, 800.0);
            assert_eq!(rect.visual_line_id, line.id);
        }
    }

    #[test]
    fn large_font_line_end_roundtrip() {
        init_qt();
        let mut layout = EditorLayout::default();
        let text = "这是一段测试文字，用来验证大字号下换行后的行尾点击定位是否正确。第二行内容继续测试换行效果。";
        let snapshot = layout
            .snapshot(
                text,
                LayoutParams {
                    width: 820.0,
                    font_size: 45.0,
                    font_family: "serif".to_string(),
                    line_spacing: 1.5,
                    text_indent: 0.0,
                    padding: 16.0,
                },
            )
            .clone();
        assert!(snapshot.lines.len() > 1, "text must wrap at fontSize=45");
        assert_line_end_roundtrip(&snapshot);
    }

    #[test]
    fn many_wrap_lines_roundtrip() {
        init_qt();
        let text = "写作者是一个强大的桌面写作工具，支持自动换行、行尾点击定位、光标动画等核心编辑功能。我们通过大量中文段落来测试自动换行后每一行的行尾光标定位是否准确。第一段测试内容结束。第二段继续测试更长的文本内容，确保每一行都能正确地进行光标位置计算和逆向映射。";
        let mut layout = EditorLayout::default();
        let snapshot = layout
            .snapshot(
                text,
                LayoutParams {
                    width: 400.0,
                    font_size: 24.0,
                    font_family: "serif".to_string(),
                    line_spacing: 1.5,
                    text_indent: 0.0,
                    padding: 16.0,
                },
            )
            .clone();
        assert!(
            snapshot.lines.len() >= 3,
            "text must wrap into >= 3 lines, got {}",
            snapshot.lines.len()
        );
        assert_line_end_roundtrip(&snapshot);
        for (idx, line) in snapshot.lines.iter().enumerate() {
            if line.start == line.end || line.para_text.is_empty() {
                continue;
            }
            let x_end = qtextlayout_cursor_to_x_on_line(
                &line.para_text,
                line.end,
                line.para_start,
                snapshot.font_size as f64,
                &snapshot.font_family,
                line.line_wrap_width + line.line_indent_x,
                line.para_indent,
                line.qtextline_idx,
                true,
            );
            assert!(
                x_end > 1.0,
                "line {} end x must be > 1.0, got {:.4} (range {}..{})",
                idx,
                x_end,
                line.start,
                line.end
            );
            let roundtrip = qtextlayout_x_to_cursor_on_line(
                &line.para_text,
                x_end,
                line.para_start,
                snapshot.font_size as f64,
                &snapshot.font_family,
                line.line_wrap_width + line.line_indent_x,
                line.para_indent,
                line.qtextline_idx,
            );
            assert_eq!(
                roundtrip, line.end,
                "line {}: xToCursor(cursorToX(line.end={})) returned {}",
                idx, line.end, roundtrip
            );
        }
    }

    fn params_large(width: f64) -> LayoutParams {
        LayoutParams {
            width,
            font_size: 45.0,
            font_family: "serif".to_string(),
            line_spacing: 1.5,
            text_indent: 32.0,
            padding: 16.0,
        }
    }

    #[test]
    fn large_font_45_wide_window_long_paragraph_line_end() {
        init_qt();
        let text = "写作者是一个强大的桌面写作工具，支持自动换行、行尾点击定位、光标动画等核心编辑功能。我们通过大量中文段落来测试自动换行后每一行的行尾光标定位是否准确。大字号下每行能容纳的字数更少，所以换行更频繁，这正是容易出问题的场景。";
        let mut layout = EditorLayout::default();
        let snapshot = layout.snapshot(text, params_large(820.0)).clone();
        assert!(
            snapshot.lines.len() >= 3,
            "fontSize=45 must produce >= 3 wrapped lines, got {}",
            snapshot.lines.len()
        );
        assert_line_end_roundtrip(&snapshot);
    }

    #[test]
    fn large_font_45_wide_window_multi_paragraph() {
        init_qt();
        let text = "第一段：这是大字号多段落测试，每一段都会独立换行。\n第二段：继续测试大字号下的多段落换行效果，确保段落之间的边界不会出错。\n第三段：最后一段，验证整个文档的换行一致性。";
        let mut layout = EditorLayout::default();
        let snapshot = layout.snapshot(text, params_large(820.0)).clone();
        assert!(
            snapshot.lines.len() >= 4,
            "multi-paragraph fontSize=45 must produce >= 4 lines, got {}",
            snapshot.lines.len()
        );
        assert_line_end_roundtrip(&snapshot);
    }

    #[test]
    fn large_font_45_soft_wrap_line_end_click() {
        init_qt();
        let text = "这是一个用来测试软换行后行尾点击定位的段落。当我们点击某个换行后的行尾位置时，光标的target_x不应该回到行首缩进位置。";
        let mut layout = EditorLayout::default();
        let snapshot = layout.snapshot(text, params_large(600.0)).clone();
        assert!(
            snapshot.lines.len() >= 2,
            "must wrap at width=600 fontSize=45"
        );
        for line in &snapshot.lines {
            if line.start == line.end || line.para_text.is_empty() {
                continue;
            }
            let x_end = qtextlayout_cursor_to_x_on_line(
                &line.para_text,
                line.end,
                line.para_start,
                snapshot.font_size as f64,
                &snapshot.font_family,
                line.line_wrap_width + line.line_indent_x,
                line.para_indent,
                line.qtextline_idx,
                true,
            );
            assert!(
                x_end > 1.0,
                "soft-wrap line end x must be > 1.0: line_id={}, range={}..{}, x_end={:.4}",
                line.id,
                line.start,
                line.end,
                x_end
            );
            let rect = caret_rect(&snapshot, line.end, CaretAffinity::Upstream, 0.0, 800.0);
            assert!(
                rect.x > 1.0,
                "caret_rect(line.end, Upstream).x must be > 1.0: line_id={}, rect.x={:.4}",
                line.id,
                rect.x
            );
            assert_eq!(
                rect.visual_line_id, line.id,
                "caret_rect(line.end, Upstream) must stay on source line: line_id={}, got visual_line_id={}",
                line.id, rect.visual_line_id
            );
        }
    }

    #[test]
    fn visual_line_qchar_boundary_matches_qt() {
        init_qt();
        let text =
            "写作者是一个强大的桌面写作工具，支持自动换行、行尾点击定位、光标动画等核心编辑功能。";
        let snapshot = snapshot_for(text, 200.0);
        assert!(snapshot.lines.len() >= 2, "must wrap at width=200");
        for line in &snapshot.lines {
            if line.para_text.is_empty() {
                continue;
            }
            let para: QString = line.para_text.to_string().into();
            let fs = snapshot.font_size as f32;
            let ff: QString = snapshot.font_family.to_string().into();
            let pw = line.line_wrap_width + line.line_indent_x;
            let pi = line.para_indent;
            let qtl = line.qtextline_idx;
            let (qt_text_start, qt_text_end) = cpp!(unsafe [
                para as "QString", fs as "float", ff as "QString",
                pw as "double", pi as "double", qtl as "int"
            ] -> (i32, i32) as "std::pair<int,int>" {
                QFont font(ff);
                font.setPixelSize(static_cast<int>(fs));
                QTextLayout layout(para, font);
                QTextOption option;
                option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
                layout.setTextOption(option);
                layout.beginLayout();
                int cur = 0;
                bool first = true;
                while (true) {
                    QTextLine line = layout.createLine();
                    if (!line.isValid()) break;
                    double lineWrap = first ? (pw - pi) : pw;
                    line.setLineWidth(lineWrap);
                    if (cur == qtl) {
                        int ts = line.textStart();
                        int te = ts + line.textLength();
                        layout.endLayout();
                        return std::make_pair(ts, te);
                    }
                    first = false;
                    cur++;
                }
                layout.endLayout();
                return std::make_pair(-1, -1);
            });
            assert_eq!(
                qt_text_start, line.para_qchar_start as i32,
                "VisualLine para_qchar_start mismatch on line {}: expected {} got {}",
                line.id, qt_text_start, line.para_qchar_start
            );
            assert_eq!(
                qt_text_end, line.para_qchar_end as i32,
                "VisualLine para_qchar_end mismatch on line {}: expected {} got {}",
                line.id, qt_text_end, line.para_qchar_end
            );
        }
    }

    #[test]
    fn cursor_7306_7065_style_target_x_not_indent() {
        init_qt();
        let text = "这是模拟实机日志中出现的问题段落。当光标位于某个visual line的中间位置时，target_x不应该坍缩到行首缩进位置61.0。通过fontSize=45和长段落来复现这个场景。";
        let mut layout = EditorLayout::default();
        let snapshot = layout
            .snapshot(
                text,
                LayoutParams {
                    width: 820.0,
                    font_size: 45.0,
                    font_family: "serif".to_string(),
                    line_spacing: 1.5,
                    text_indent: 32.0,
                    padding: 16.0,
                },
            )
            .clone();
        assert!(snapshot.lines.len() >= 3);
        for line in &snapshot.lines {
            if line.start == line.end || line.para_text.is_empty() {
                continue;
            }
            let mid_byte = line.start + (line.end - line.start) / 2;
            let mid_cursor = text.floor_char_boundary(mid_byte);
            let mid_cursor = mid_cursor.max(line.start).min(line.end);
            if mid_cursor == line.start || mid_cursor == line.end {
                continue;
            }
            let mid_rect = caret_rect(&snapshot, mid_cursor, CaretAffinity::Downstream, 0.0, 800.0);
            assert!(
                mid_rect.x > 1.0,
                "mid-cursor target_x must not collapse to indent: line_id={}, mid_cursor={}, rect.x={:.4}",
                line.id, mid_cursor, mid_rect.x
            );
            let x_end = qtextlayout_cursor_to_x_on_line(
                &line.para_text,
                line.end,
                line.para_start,
                snapshot.font_size as f64,
                &snapshot.font_family,
                line.line_wrap_width + line.line_indent_x,
                line.para_indent,
                line.qtextline_idx,
                true,
            );
            assert!(
                x_end > 1.0,
                "line-end x must not collapse to indent: line_id={}, x_end={:.4}",
                line.id,
                x_end
            );
        }
    }

    #[test]
    fn x_end_trailing_consistent_with_recomputed() {
        init_qt();
        let text = "验证x_end_trailing缓存值与重新计算值一致。这是测试段落，用来确保布局缓存不会导致光标位置偏差。";
        let mut layout = EditorLayout::default();
        let snapshot = layout
            .snapshot(
                text,
                LayoutParams {
                    width: 600.0,
                    font_size: 32.0,
                    font_family: "serif".to_string(),
                    line_spacing: 1.5,
                    text_indent: 0.0,
                    padding: 16.0,
                },
            )
            .clone();
        assert!(snapshot.lines.len() >= 2);
        for line in &snapshot.lines {
            if line.start == line.end || line.para_text.is_empty() {
                continue;
            }
            let recomputed = qtextlayout_cursor_to_x_on_line(
                &line.para_text,
                line.end,
                line.para_start,
                snapshot.font_size as f64,
                &snapshot.font_family,
                line.line_wrap_width + line.line_indent_x,
                line.para_indent,
                line.qtextline_idx,
                true,
            );
            let cached = line.x_end_trailing;
            let diff = (recomputed - cached).abs();
            assert!(
                diff < 0.5,
                "x_end_trailing inconsistency on line {}: cached={:.4} recomputed={:.4} diff={:.4}",
                line.id,
                cached,
                recomputed,
                diff
            );
        }
    }
}
