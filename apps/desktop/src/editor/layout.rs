use cpp::cpp;
use qmetaobject::QString;

cpp! {{
    #include <QtGui/QFont>
    #include <QtGui/QFontMetricsF>
    #include <QtGui/QTextLayout>
    #include <QtGui/QTextOption>
    #include <QGuiApplication>
    #include <vector>

    struct EditorLayoutEntry {
        int qcharStart;
        double width;
        double xPos;
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
            int line_len = line.textLength();
            if (qchar_count >= line_start && qchar_count <= line_start + line_len) {
                x = line.cursorToX(qchar_count - line_start);
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
                int line_len = line.textLength();
                int local_qchar = cursor_qchar - line_start;
                if (local_qchar < 0) local_qchar = 0;
                if (local_qchar > line_len) local_qchar = line_len;

                if (local_qchar == line_len && line_len > 0) {
                    x = line.cursorToX(line_len - 1, QTextLine::Trailing);
                } else {
                    x = line.cursorToX(
                        local_qchar,
                        use_trailing ? QTextLine::Trailing : QTextLine::Leading
                    );
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
                int line_len = line.textLength();
                int local_qchar = line.xToCursor(x);
                if (local_qchar < 0) local_qchar = 0;
                if (local_qchar > line_len) local_qchar = line_len;
                target_idx = line_start + local_qchar;
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
            e.width = line.naturalTextWidth();
            e.xPos = first ? indent_w : 0.0;
            g_editor_layout_buf.push_back(e);
            first = false;
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
                    double x1 = line.cursorToX(i - line_start);
                    double x2 = line.cursorToX(i - line_start + 1);
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
            line.line_indent_x,
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

            let byte_off = qchar_offset_to_byte_offset(paragraph_text, qchar_off);
            let abs_start = para_start + byte_off;
            let para_qchar_end = if line_idx + 1 < line_count {
                let next_idx = line_idx + 1;
                cpp!(unsafe [next_idx as "int"] -> usize as "qulonglong" {
                    if (next_idx >= 0 && next_idx < static_cast<int>(g_editor_layout_buf.size())) {
                        return static_cast<qulonglong>(g_editor_layout_buf[next_idx].qcharStart);
                    }
                    return 0;
                })
            } else {
                byte_offset_to_qchar_offset(paragraph_text, paragraph_text.len())
            };
            let abs_end = if line_idx + 1 < line_count {
                para_start + qchar_offset_to_byte_offset(paragraph_text, para_qchar_end)
            } else {
                paragraph_text_end
            };
            let is_first = line_idx == 0;

            result.push(VisualLine {
                id: line_id,
                start: abs_start,
                end: abs_end,
                hard_break: hard_break && abs_end == paragraph_text_end,
                x: padding + x_off,
                y,
                width: line_w,
                height: line_height,
                para_text: paragraph_text.to_string(),
                para_start,
                qtextline_idx: line_idx as i32,
                para_qchar_start: qchar_off,
                para_qchar_end,
                line_wrap_width: if is_first {
                    available - indent
                } else {
                    available
                },
                line_indent_x: if is_first { indent } else { 0.0 },
            });
            line_id += 1;
            y += line_height;
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
        None => (lines.len() - 1, lines.last().unwrap()),
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
            };
            &fallback
        }
    };

    let cursor_x = calculate_cursor_x_for_line(line, cursor_byte, affinity, snapshot);
    let (cursor_y_doc, cursor_h) =
        cursor_rect_for_line(line, snapshot.font_size as f64, &snapshot.font_family);
    let cursor_y = cursor_y_doc - scroll_y;
    let visible = cursor_y + cursor_h > 0.0 && cursor_y < viewport_h.max(1.0);

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
        line.line_indent_x,
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
        let use_trailing = affinity == CaretAffinity::Upstream && cursor == line.end;
        let paragraph_wrap_w = line.line_wrap_width + line.line_indent_x;
        line.x
            + qtextlayout_cursor_to_x_on_line(
                &line.para_text,
                cursor,
                line.para_start,
                snapshot.font_size as f64,
                &snapshot.font_family,
                paragraph_wrap_w,
                line.line_indent_x,
                line.qtextline_idx,
                use_trailing,
            )
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
fn get_font_ascent(font_family: &str, font_size: f32) -> f64 {
    let family = QString::from(font_family);
    cpp!(unsafe [family as "QString", font_size as "float"] -> f64 as "double" {
        QFont font(family);
        font.setPixelSize(font_size);
        QFontMetricsF metrics(font);
        return metrics.ascent();
    })
}

#[cfg(test)]
fn get_font_ascent(_font_family: &str, font_size: f32) -> f64 {
    font_size as f64 * 0.8
}

#[cfg(not(test))]
fn get_font_descent(font_family: &str, font_size: f32) -> f64 {
    let family = QString::from(font_family);
    cpp!(unsafe [family as "QString", font_size as "float"] -> f64 as "double" {
        QFont font(family);
        font.setPixelSize(font_size);
        QFontMetricsF metrics(font);
        return metrics.descent();
    })
}

#[cfg(test)]
fn get_font_descent(_font_family: &str, font_size: f32) -> f64 {
    font_size as f64 * 0.2
}

pub fn cursor_height_for_line(font_size: f64, font_family: &str) -> f64 {
    let ascent = get_font_ascent(font_family, font_size as f32);
    let descent = get_font_descent(font_family, font_size as f32);
    ascent + descent
}

pub fn cursor_rect_for_line(line: &VisualLine, font_size: f64, font_family: &str) -> (f64, f64) {
    let raw_h = cursor_height_for_line(font_size, font_family);
    let h = raw_h.min(line.height * 0.84);
    let top_y = line.y + (line.height - h) / 2.0;
    (top_y, h)
}

pub fn cursor_top_y(line: &VisualLine, font_size: f64, font_family: &str) -> f64 {
    cursor_rect_for_line(line, font_size, font_family).0
}

pub fn text_baseline_y(line: &VisualLine, font_size: f64, font_family: &str) -> f64 {
    let ascent = get_font_ascent(font_family, font_size as f32);
    let descent = get_font_descent(font_family, font_size as f32);
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
                line.line_indent_x,
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
            let roundtrip = qtextlayout_x_to_cursor_on_line(
                &line.para_text,
                x,
                line.para_start,
                snapshot.font_size as f64,
                &snapshot.font_family,
                line.line_wrap_width + line.line_indent_x,
                line.line_indent_x,
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
}
