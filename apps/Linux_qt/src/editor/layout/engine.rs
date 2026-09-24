use cpp::cpp;
use qmetaobject::QString;

use super::canonical_snapshot::{
    CanonicalLineSnapshot, CanonicalParagraphSnapshot, CanonicalClusterSnapshot, CursorXMapEntry,
};
use super::types::VisualLine;

// ── Qt 文本布局模块：排版引擎 ──
//
// 段落拆分、wrap、indent、line geometry、一次排版（prepare_paragraph_layout_core 等
// C++ 排版核心、prepare_paragraph_visual_snapshot）、几何 helper、坐标转换、
// run_on_qt_thread。
//
// 线程安全：`g_editor_layout_buf` 和 `g_glyph_buf` 为 thread_local，
// 仅在 GUI 线程中使用，不跨线程共享。

cpp! {{
    #include <QtGlobal>
    #include <QtGui/QFont>
    #include <QtGui/QFontMetricsF>
    #include <QtGui/QPainter>
    #include <QtGui/QTextLayout>
    #include <QtGui/QTextOption>
    #include <QGuiApplication>
    #include <QStringList>
    #include <vector>

    // ── Forward declarations from qt_cache.rs ──
    // cpp_build 按 mod 声明顺序拼接 cpp! {{}} 块到同一 C++ 文件，
    // engine 在 qt_cache 之前，需前向声明 qt_cache 定义的 generation cache helper。
    QTextLayout* get_paragraph_layout(uint64_t gen, int slot);
    void set_paragraph_layout_slot_gen(uint64_t gen, int cache_slot, QTextLayout* layout);
    void set_null_paragraph_layout_slot_gen(uint64_t gen, int cache_slot);

    // Issue #693 评论 5690222437: 统一 glyphRuns 提取，显式要求
    // RetrieveStringIndexes，确保 stringIndexes() 非空，使吞字/吐字事务
    // (InsertReveal / DeleteConceal / Reflow*) 能拿到 CanonicalClusterSnapshot
    // 的 cluster。Linux_Qt 最低版本 Qt 6.7，不再保留 Qt < 6.5 旧分支。
    static QList<QGlyphRun> sujianGlyphRuns(const QTextLine& line) {
        return line.glyphRuns(
            -1,
            -1,
            QTextLayout::RetrieveGlyphIndexes |
            QTextLayout::RetrieveGlyphPositions |
            QTextLayout::RetrieveStringIndexes
        );
    }

    /// 单行排版结果 — 跨 C++/Rust 边界的数据结构。
    ///
    /// 所有 QChar index 为 UTF-16 code unit offset（与 QTextLayout API 一致），
    /// 传入 Core 前必须转换为 UTF-8 byte offset。
    ///
    /// - `qcharStart/qcharEnd`：该行在段落中的 QChar 范围（半开区间）
    /// - `width`：行宽（逻辑像素）
    /// - `xPos`：行在段落中的水平起始位置（首行缩进时 > 0）
    /// - `xEndLeading/xEndTrailing`：行尾位置（含/不含 trailing whitespace）
    /// - `naturalTextWidth`：自然文本宽度（不含对齐拉伸）
    /// - `ascent/descent`：字体度量（逻辑像素，用于光标高度计算）
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
        // Issue #658: canonical 行高 — 与 Rust 侧 actual_line_h 完全一致的行高，
        // 由 C++ 排版时计算并 setPosition，Rust 侧直接读取使用，
        // 确保 QTextLayout 内部行位置和 VisualLine.y 共享同一份几何。
        double lineHeight;
    };
    thread_local std::vector<EditorLayoutEntry> g_editor_layout_buf;

    /// 单个 glyph 信息 — 跨 C++/Rust 边界的数据结构。
    ///
    /// - `stringIndex`：该 glyph 对应字符在源字符串中的 QChar index（UTF-16 code unit offset）
    /// - `xPos`：glyph 左边缘 x 坐标（逻辑像素，文档坐标系）
    /// - `width`：glyph 前进宽度（逻辑像素）
    /// - `glyphIndex`：QGlyphRun 中的实际 glyph 索引（用于 ligature 拆分后的精确定位）
    /// - `rawFontKey`：QRawFont 的字体族名称（用于 font_id 匹配）
    struct GlyphEntry {
        int stringIndex;    // QChar index in the source string
        double xPos;        // Left edge of the glyph
        double width;       // Advance width of the glyph
        unsigned int glyphIndex;  // Real glyph index from QGlyphRun
        char rawFontKey[256];     // Font family name from QRawFont (for font_id)
    };
    thread_local std::vector<GlyphEntry> g_glyph_buf;

    // Issue #658 评论 5621512329 问题 2: 共享的段落排版核心。
    // editor_layout_lines（静态正文）和 editor_prepare_paragraph_visual_snapshot
    //（动画/IME）共用此核心，消除两套重复的 QTextLayout 创建/排版代码。
    // 创建 QTextLayout，执行 beginLayout/createLine/setLineWidth/setPosition/endLayout，
    // 返回 layout 指针和 textLines 列表。调用方负责在用完后 delete layout
    // 或存入 generation cache。
    struct PreparedParagraphLayoutCore {
        QTextLayout* layout;
        QVector<QTextLine> textLines;
    };

    static PreparedParagraphLayoutCore prepare_paragraph_layout_core(
        const QString& paraText, const QFont& font,
        double wrap_w, double indent_w, double fs, double line_spacing, double metrics_h
    ) {
        PreparedParagraphLayoutCore result;
        result.layout = new QTextLayout(paraText, font);
        QTextOption option;
        option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
        result.layout->setTextOption(option);
        result.layout->setCacheEnabled(true);
        result.layout->beginLayout();

        bool first = true;
        // local_y 按 canonical 行高累加，与 Rust 侧 VisualLine.y 一致。
        // 给每个 QTextLine 写入最终 setPosition，使 layout 完整排版。
        double local_y = 0.0;
        while (true) {
            QTextLine line = result.layout->createLine();
            if (!line.isValid()) break;
            double lineWrap = first ? (wrap_w - indent_w) : wrap_w;
            line.setLineWidth(lineWrap);
            // canonical 行高公式与 Rust 侧 actual_line_h 完全一致：
            // max(font_size * line_spacing, font_size + 4.0, metrics_h, line.ascent()+line.descent())
            double qt_metrics_h = line.ascent() + line.descent();
            double canonical_line_h = std::max(fs * line_spacing,
                std::max(fs + 4.0, std::max(metrics_h, qt_metrics_h)));
            double line_x = first ? indent_w : 0.0;
            line.setPosition(QPointF(line_x, local_y));
            local_y += canonical_line_h;
            result.textLines.push_back(line);
            first = false;
        }
        result.layout->endLayout();
        return result;
    }

    // Issue #658 评论 5622829886 问题 3: 删除 editor_layout_lines 旧 fallback 路径。
    // 静态正文排版已收口到 prepare_document_visual_snapshot（与动画/IME 同一入口），
    // 共用 prepare_paragraph_layout_core 排版核心，不再保留第二套可回退路径。

    int editor_layout_entry_count() {
        return static_cast<int>(g_editor_layout_buf.size());
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

        g_glyph_buf.clear();
        int cur_idx = 0;
        bool first = true;
        while (true) {
            QTextLine line = layout.createLine();
            if (!line.isValid()) break;
            double lineWrap = first ? (paragraph_wrap_w - indent_w) : paragraph_wrap_w;
            line.setLineWidth(lineWrap);
            if (cur_idx == qtextline_idx) {
                // Use glyphRuns() for accurate glyph positions.
                // This handles emoji, combining characters, ligatures, etc.
                const auto glyphRuns = sujianGlyphRuns(line);

                // Qt 6.5+: stringIndexes() provides precise glyph→string index mapping
                for (const auto& run : glyphRuns) {
                    const auto& positions = run.positions();
                    const auto& stringIndexes = run.stringIndexes();
                    const auto& glyphIndexes = run.glyphIndexes();
                    int count = positions.size();

                    // Extract real raw font family for font_id
                    QRawFont rawFont = run.rawFont();
                    QString rawFontFamily = rawFont.familyName();
                    QByteArray rawFontKeyBytes = rawFontFamily.toUtf8();

                    for (int i = 0; i < count; i++) {
                        int strIdx = (i < stringIndexes.size())
                            ? stringIndexes[i] : -1;

                        // Skip glyphs outside the requested range
                        if (strIdx < 0 || strIdx < range_qchar_start || strIdx >= range_qchar_end) {
                            continue;
                        }

                        double x = positions[i].x();
                        double w = 0.0;

                        // Calculate width: use next glyph position or cursorToX fallback
                        if (i + 1 < count) {
                            int nextStrIdx = (i + 1 < stringIndexes.size())
                                ? stringIndexes[i + 1] : -1;
                            if (nextStrIdx >= range_qchar_start && nextStrIdx < range_qchar_end) {
                                w = positions[i + 1].x() - x;
                            } else {
                                // Next glyph is outside range — use cursorToX for boundary
                                w = line.cursorToX(strIdx + 1) - x;
                            }
                        } else {
                            // Last glyph in run — use cursorToX for trailing edge
                            w = line.cursorToX(strIdx + 1) - x;
                        }

                        if (w < 0) w = -w; // RTL text
                        if (w < 0.01) w = line.cursorToX(strIdx + 1) - line.cursorToX(strIdx);

                        GlyphEntry e;
                        e.stringIndex = strIdx;
                        e.xPos = x;
                        e.width = w;
                        // Real glyph index from QGlyphRun
                        e.glyphIndex = (i < glyphIndexes.size())
                            ? glyphIndexes[i] : 0;
                        // Real raw font family for font_id
                        memset(e.rawFontKey, 0, sizeof(e.rawFontKey));
                        if (rawFontKeyBytes.size() > 0) {
                            int copyLen = rawFontKeyBytes.size();
                            if (copyLen > (int)sizeof(e.rawFontKey) - 1)
                                copyLen = (int)sizeof(e.rawFontKey) - 1;
                            memcpy(e.rawFontKey, rawFontKeyBytes.constData(), copyLen);
                        }
                        g_glyph_buf.push_back(e);
                    }
                }

                // Sort by string index to ensure consistent ordering
                std::sort(g_glyph_buf.begin(), g_glyph_buf.end(),
                    [](const GlyphEntry& a, const GlyphEntry& b) {
                        return a.stringIndex < b.stringIndex;
                    });

                // Remove duplicates (same stringIndex can appear in different runs)
                g_glyph_buf.erase(
                    std::unique(g_glyph_buf.begin(), g_glyph_buf.end(),
                        [](const GlyphEntry& a, const GlyphEntry& b) {
                            return a.stringIndex == b.stringIndex;
                        }),
                    g_glyph_buf.end());

                break;
            }
            first = false;
            cur_idx++;
        }
        layout.endLayout();
    }

    // Draw a full line of text using QTextLine::draw().
    // This ensures the text rendering uses the same shaping data as
    // cursorToX() / xToCursor(), fixing mixed-script cursor issues
    // (e.g. "]\"" where cursor lands inside the Chinese quote).
    // Per-run data for QGlyphRun-level extraction
    struct ShapedRunEntry {
        int runIndex;
        int glyphCount;
        int stringStart;
        int stringEnd;
        bool isRTL;
        bool hasUnderline;
        char rawFontFamily[256];
        char rawFontStyle[128];
        int rawFontWeight;
        int rawFontPixelSize;
        double baselineY;
        double visualX;
        double visualY;
        double visualW;
        double visualH;
        double textureTranslateX;
        double textureTranslateY;
        double lineY;
    };
    thread_local std::vector<ShapedRunEntry> g_shaped_run_buf;

    // Per-glyph data within a specific run
    struct RunGlyphEntry {
        int runIndex;
        unsigned int glyphIndex;
        double positionX;
        double positionY;
        int stringIndex;
        double advanceWidth;
    };
    thread_local std::vector<RunGlyphEntry> g_run_glyph_buf;

    void editor_layout_shaped_runs_on_line(
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

        g_shaped_run_buf.clear();
        g_run_glyph_buf.clear();
        int cur_idx = 0;
        bool first = true;
        while (true) {
            QTextLine line = layout.createLine();
            if (!line.isValid()) break;
            double lineWrap = first ? (paragraph_wrap_w - indent_w) : paragraph_wrap_w;
            line.setLineWidth(lineWrap);
            if (cur_idx == qtextline_idx) {
                const auto glyphRuns = sujianGlyphRuns(line);
                double lineY = line.y();
                double lineH = line.height();
                double lineAscent = line.ascent();

                int runIdx = 0;
                for (const auto& run : glyphRuns) {
                    const auto& positions = run.positions();
                    const auto& glyphIndexes = run.glyphIndexes();
                    const auto& stringIndexes = run.stringIndexes();
                    int count = positions.size();
                    if (count == 0) { runIdx++; continue; }

                    QRawFont rawFont = run.rawFont();
                    QString rawFontFamily = rawFont.familyName();
                    QByteArray rawFontKeyBytes = rawFontFamily.toUtf8();

                    // Extract font properties for stable cache key
                    QFont derivedFont;
                    derivedFont.setFamily(rawFontFamily);
                    derivedFont.setPixelSize(static_cast<int>(fs));
                    QString rawFontStyle;
                    int rawFontWeight = derivedFont.weight();

                    // Compute run-level string range and real visual bounds
                    // using QRawFont::boundingRect(glyphIndex) + glyph position
                    int strStart = INT_MAX;
                    int strEnd = 0;
                    double unionMinX = 1e9, unionMinY = 1e9;
                    double unionMaxX = -1e9, unionMaxY = -1e9;
                    for (int i = 0; i < count; i++) {
                        int si = (i < stringIndexes.size()) ? stringIndexes[i] : -1;
                        if (si >= 0) {
                            if (si < strStart) strStart = si;
                            if (si + 1 > strEnd) strEnd = si + 1;
                        }
                        quint32 gIdx = (i < glyphIndexes.size()) ? glyphIndexes[i] : 0;
                        QRectF glyphBounds = rawFont.boundingRect(gIdx);
                        double gx = positions[i].x();
                        double gy = positions[i].y();
                        double gLeft   = gx + glyphBounds.left();
                        double gRight  = gx + glyphBounds.right();
                        double gTop    = gy + glyphBounds.top();
                        double gBottom = gy + glyphBounds.bottom();
                        if (gLeft   < unionMinX) unionMinX = gLeft;
                        if (gRight  > unionMaxX) unionMaxX = gRight;
                        if (gTop    < unionMinY) unionMinY = gTop;
                        if (gBottom > unionMaxY) unionMaxY = gBottom;
                    }
                    if (strStart == INT_MAX) strStart = 0;
                    if (strEnd == 0) strEnd = strStart;

                    // Filter: skip runs entirely outside requested range
                    if (strEnd <= range_qchar_start || strStart >= range_qchar_end) {
                        runIdx++; continue;
                    }

                    // Anti-aliasing margin (1px on each side in logical coords)
                    double aaMargin = 1.0;
                    double runW = (unionMaxX - unionMinX) + aaMargin * 2.0;
                    double runH = (unionMaxY - unionMinY) + aaMargin * 2.0;
                    if (runW < 0.01 && count > 0) runW = 10.0;
                    if (runH < 0.01 && count > 0) runH = lineH;

                    // Texture translation: shift glyph positions so union bounds
                    // top-left maps to (aaMargin, aaMargin) in the texture
                    double texTransX = -unionMinX + aaMargin;
                    double texTransY = -unionMinY + aaMargin;

                    ShapedRunEntry re;
                    re.runIndex = runIdx;
                    re.glyphCount = count;
                    re.stringStart = strStart;
                    re.stringEnd = strEnd;
                    re.isRTL = run.isRightToLeft();
                    re.hasUnderline = false;
                    memset(re.rawFontFamily, 0, sizeof(re.rawFontFamily));
                    if (rawFontKeyBytes.size() > 0) {
                        int copyLen = rawFontKeyBytes.size();
                        if (copyLen > (int)sizeof(re.rawFontFamily) - 1)
                            copyLen = (int)sizeof(re.rawFontFamily) - 1;
                        memcpy(re.rawFontFamily, rawFontKeyBytes.constData(), copyLen);
                    }
                    memset(re.rawFontStyle, 0, sizeof(re.rawFontStyle));
                    re.rawFontWeight = rawFontWeight;
                    re.rawFontPixelSize = static_cast<int>(fs);
                    re.baselineY = lineY + lineAscent;
                    re.visualX = unionMinX - aaMargin;
                    re.visualY = unionMinY - aaMargin;
                    re.visualW = runW;
                    re.visualH = runH;
                    re.textureTranslateX = texTransX;
                    re.textureTranslateY = texTransY;
                    re.lineY = lineY;
                    g_shaped_run_buf.push_back(re);

                    // Extract per-glyph data
                    for (int i = 0; i < count; i++) {
                        RunGlyphEntry ge;
                        ge.runIndex = runIdx;
                        ge.glyphIndex = (i < glyphIndexes.size()) ? glyphIndexes[i] : 0;
                        ge.positionX = positions[i].x();
                        ge.positionY = positions[i].y();
                        ge.stringIndex = (i < stringIndexes.size()) ? stringIndexes[i] : -1;
                        ge.advanceWidth = 0.0;
                        if (i + 1 < count) {
                            ge.advanceWidth = positions[i + 1].x() - positions[i].x();
                        }
                        if (ge.advanceWidth < 0.01 && ge.stringIndex >= 0) {
                            double cxNext = line.cursorToX(ge.stringIndex + 1);
                            double cxThis = line.cursorToX(ge.stringIndex);
                            ge.advanceWidth = cxNext - cxThis;
                        }
                        if (ge.advanceWidth < 0) ge.advanceWidth = -ge.advanceWidth;
                        g_run_glyph_buf.push_back(ge);
                    }

                    runIdx++;
                }
                break;
            }
            first = false;
            cur_idx++;
        }
        layout.endLayout();
    }

    // Qt mature route: render a single QTextLine to a QImage using QTextLine::draw().
    // This produces a line-level visual snapshot that can be UV-clipped to extract
    // individual glyph runs, clusters, or text segments — without re-laying out text
    // for each animation texture. Core principle: layout once, snapshot once,
    // animation phase no longer understands text.
    void editor_render_line_to_image(
        QImage* img, const QString& paraText,
        double fs, const QString& ff,
        double paragraph_wrap_w, double indent_w, int qtextline_idx,
        double dpr, const QColor& textColor
    ) {
        if (!img) return;
        QPainter painter(img);
        painter.setRenderHint(QPainter::TextAntialiasing, true);
        painter.scale(dpr, dpr);

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
                painter.setPen(QPen(textColor));
                QPointF pos(0, line.ascent());
                line.draw(&painter, pos);
                break;
            }
            first = false;
            cur_idx++;
        }
        layout.endLayout();
    }

    void editor_draw_line_text(
        QPainter* painter, const QString& paraText,
        double fs, const QString& ff,
        double paragraph_wrap_w, double indent_w, int qtextline_idx,
        double x, double baseline_y, const QColor& textColor
    ) {
        if (!painter) return;
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
                // Use QTextLine::draw() with the same layout that cursorToX uses.
                // This guarantees cursor position and text rendering are consistent.
                painter->setPen(QPen(textColor));
                QPointF pos(x, baseline_y - line.ascent());
                line.draw(painter, pos);
                break;
            }
            first = false;
            cur_idx++;
        }
        layout.endLayout();
    }

    // ── Canonical paragraph visual snapshot ──
    // One QTextLayout per paragraph: layout, glyphRuns, clusters, line images,
    // cursor data — all from the same instance. No re-layout after this.

    struct CanonicalLineEntry {
        int qcharStart;
        int qcharEnd;
        double xPos;
        double width;
        double height;
        double ascent;
        double descent;
        double y;
        double xEndLeading;
        double xEndTrailing;
        int clusterStartIndex;
        int clusterCount;
        int imagePhysW;
        int imagePhysH;
        int cursorXMapStart;
        int cursorXMapCount;
    };

    struct CursorXMapEntry {
        int qcharPos;
        double xLeading;
        double xTrailing;
    };
    thread_local std::vector<CursorXMapEntry> g_cursor_x_map_buf;

    struct CanonicalClusterEntry {
        int qcharStart;
        int qcharEnd;
        double sourceRectX;
        double sourceRectY;
        double sourceRectW;
        double sourceRectH;
        int glyphCount;
        int glyphStartIndex;
        char rawFontFingerprint[256];
        bool isRTL;
        unsigned int firstGlyphIndex;
    };

    struct CanonicalClusterGlyphEntry {
        unsigned int glyphIndex;
        double positionX;
        double positionY;
        int stringIndex;
    };

    thread_local std::vector<CanonicalLineEntry> g_canonical_line_buf;
    thread_local std::vector<CanonicalClusterEntry> g_canonical_cluster_buf;
    thread_local std::vector<CanonicalClusterGlyphEntry> g_canonical_cluster_glyph_buf;
    thread_local std::vector<QImage> g_canonical_line_images;

    // Issue #658 评论 5624570557 问题 1+2: 从已有 QTextLine 提取动画视觉。
    // 不创建新的 QTextLayout，直接从已排好的 line 提取 QImage/glyphRuns/cluster。
    // 按 (generation, cache_slot, qtextline_idx) 读取现成 QTextLine，
    // 避免动画 old 帧重新排版。
    // 返回提取的 line 数据到 g_canonical_line_buf/g_canonical_cluster_buf/g_canonical_cluster_glyph_buf。
    static void extract_animation_visuals_from_existing_line(
        uint64_t gen, int slot, int qtextline_idx,
        double dpr, const QColor& textColor
    ) {
        QTextLayout* layout = get_paragraph_layout(gen, slot);
        if (!layout) return;
        if (qtextline_idx < 0 || qtextline_idx >= layout->lineCount()) return;

        QTextLine line = layout->lineAt(qtextline_idx);
        if (!line.isValid()) return;

        // 清空之前的 buffers
        g_canonical_line_buf.clear();
        g_canonical_cluster_buf.clear();
        g_canonical_cluster_glyph_buf.clear();
        g_canonical_line_images.clear();

        CanonicalLineEntry entry;
        entry.qcharStart = line.textStart();
        entry.qcharEnd = line.textStart() + line.textLength();
        entry.xPos = line.x();
        entry.width = line.naturalTextWidth();
        entry.height = line.height();
        entry.ascent = line.ascent();
        entry.descent = line.descent();
        entry.y = line.y();
        entry.xEndLeading = line.cursorToX(entry.qcharEnd, QTextLine::Leading) - line.x();
        entry.xEndTrailing = line.cursorToX(entry.qcharEnd, QTextLine::Trailing) - line.x();

        double logical_w = line.naturalTextWidth();
        double logical_h = line.height();
        int phys_w = (int)ceil(logical_w * dpr);
        int phys_h = (int)ceil(logical_h * dpr);

        // 1. 绘制到 QImage
        if (phys_w > 0 && phys_h > 0 && phys_w <= 8192 && phys_h <= 4096) {
            QImage img(phys_w, phys_h, QImage::Format_ARGB32_Premultiplied);
            img.setDevicePixelRatio(dpr);
            img.fill(Qt::transparent);

            QPainter painter(&img);
            painter.setRenderHint(QPainter::TextAntialiasing, true);
            painter.setPen(QPen(textColor));
            QPointF pos(-line.x(), -line.y());
            line.draw(&painter, pos);

            entry.imagePhysW = phys_w;
            entry.imagePhysH = phys_h;
            g_canonical_line_images.push_back(img);
        } else {
            entry.imagePhysW = 0;
            entry.imagePhysH = 0;
            g_canonical_line_images.push_back(QImage());
        }

        int clusterStartIdx = (int)g_canonical_cluster_buf.size();

        // 2. 提取 glyphRuns 和 clusters
        const auto glyphRuns = sujianGlyphRuns(line);

        // Issue #724 评论 5750911834 问题 1: 引入全行 logicalStarts。
        // 收集全行所有 glyph run 的 stringIndexes，用行内真实 cluster 边界
        // 计算 qcharStart/qcharEnd，不再用相邻 cluster 的 qcharVal 猜。
        // 这修复了跨 glyph run / ligature 的行边界错误，使 cluster 范围精确。
        std::vector<int> lineLogicalStarts;
        for (const auto& run : glyphRuns) {
            const auto& si = run.stringIndexes();
            for (int i = 0; i < (int)si.size(); i++) {
                if (si[i] >= 0) lineLogicalStarts.push_back(si[i]);
            }
        }
        std::sort(lineLogicalStarts.begin(), lineLogicalStarts.end());
        lineLogicalStarts.erase(
            std::unique(lineLogicalStarts.begin(), lineLogicalStarts.end()),
            lineLogicalStarts.end());

        for (const auto& run : glyphRuns) {
            const auto& positions = run.positions();
            const auto& glyphIndexes = run.glyphIndexes();
            const auto& stringIndexes = run.stringIndexes();
            int count = positions.size();
            if (count == 0) continue;

            QRawFont rawFont = run.rawFont();
            QString rawFontFamily = rawFont.familyName();
            QByteArray rawFontKeyBytes = rawFontFamily.toUtf8();

            int glyphBufStart = (int)g_canonical_cluster_glyph_buf.size();

            for (int gi = 0; gi < count; gi++) {
                unsigned int gIdx = (gi < glyphIndexes.size()) ? glyphIndexes[gi] : 0;
                double gx = positions[gi].x();
                double gy = positions[gi].y();
                int si = (gi < stringIndexes.size()) ? stringIndexes[gi] : -1;

                CanonicalClusterGlyphEntry ge;
                ge.glyphIndex = gIdx;
                ge.positionX = gx;
                ge.positionY = gy;
                ge.stringIndex = si;
                g_canonical_cluster_glyph_buf.push_back(ge);
            }

            // Cluster 提取逻辑（与 editor_prepare_paragraph_visual_snapshot 中相同）
            struct TempCluster {
                int qcharVal;
                int glyphStart;
                int glyphEnd;
                double visMinX, visMinY, visMaxX, visMaxY;
            };
            std::vector<TempCluster> tempClusters;

            if (count > 0) {
                int curQchar = g_canonical_cluster_glyph_buf[glyphBufStart].stringIndex;
                int clStart = 0;
                double clMinX = 1e9, clMinY = 1e9, clMaxX = -1e9, clMaxY = -1e9;

                for (int gi = 0; gi <= count; gi++) {
                    int si = (gi < count)
                        ? g_canonical_cluster_glyph_buf[glyphBufStart + gi].stringIndex
                        : INT_MAX;

                    if (gi == count || si != curQchar) {
                        if (curQchar >= 0) {
                            TempCluster tc;
                            tc.qcharVal = curQchar;
                            tc.glyphStart = clStart;
                            tc.glyphEnd = gi;
                            tc.visMinX = clMinX;
                            tc.visMinY = clMinY;
                            tc.visMaxX = clMaxX;
                            tc.visMaxY = clMaxY;
                            tempClusters.push_back(tc);
                        }
                        if (gi < count) {
                            curQchar = si;
                            clStart = gi;
                            clMinX = 1e9; clMinY = 1e9;
                            clMaxX = -1e9; clMaxY = -1e9;
                        }
                    }

                    if (gi < count && si == curQchar) {
                        unsigned int gIdx2 = g_canonical_cluster_glyph_buf[glyphBufStart + gi].glyphIndex;
                        double gx2 = g_canonical_cluster_glyph_buf[glyphBufStart + gi].positionX;
                        double gy2 = g_canonical_cluster_glyph_buf[glyphBufStart + gi].positionY;
                        QRectF gb = rawFont.boundingRect(gIdx2);
                        double gl = gx2 + gb.left();
                        double gr = gx2 + gb.right();
                        double gt = gy2 + gb.top();
                        double gbo = gy2 + gb.bottom();
                        if (gl < clMinX) clMinX = gl;
                        if (gr > clMaxX) clMaxX = gr;
                        if (gt < clMinY) clMinY = gt;
                        if (gbo > clMaxY) clMaxY = gbo;
                    }
                }
            }

            double aaMargin = 1.0;
            for (int ci = 0; ci < (int)tempClusters.size(); ci++) {
                const TempCluster& tc = tempClusters[ci];
                if (tc.qcharVal < 0) continue;

                int qcharStart = tc.qcharVal;
                int qcharEnd;
                // Issue #724 评论 5750911834 问题 1: 用全行 logicalStarts 计算真实
                // cluster 边界，不再用相邻 cluster 的 qcharVal 猜。这修复了跨
                // glyph run / ligature 的行边界错误，cluster 范围精确。
                auto lsIt = std::upper_bound(
                    lineLogicalStarts.begin(), lineLogicalStarts.end(), tc.qcharVal);
                if (lsIt != lineLogicalStarts.end()) {
                    qcharEnd = *lsIt;
                } else {
                    qcharEnd = entry.qcharEnd;
                }
                if (qcharEnd <= qcharStart) qcharEnd = qcharStart + 1;

                double srcX = (tc.visMinX - aaMargin) - line.x();
                double srcY = (tc.visMinY - aaMargin) - line.y();
                double srcW = (tc.visMaxX - tc.visMinX) + aaMargin * 2.0;
                double srcH = (tc.visMaxY - tc.visMinY) + aaMargin * 2.0;

                if (srcW < 0.01) srcW = 10.0;
                if (srcH < 0.01) srcH = line.height();

                if (srcX < 0) { srcW += srcX; srcX = 0; }
                if (srcY < 0) { srcH += srcY; srcY = 0; }
                if (srcX + srcW > logical_w) srcW = logical_w - srcX;
                if (srcY + srcH > logical_h) srcH = logical_h - srcY;

                CanonicalClusterEntry ce;
                ce.qcharStart = qcharStart;
                ce.qcharEnd = qcharEnd;
                ce.sourceRectX = srcX * dpr;
                ce.sourceRectY = srcY * dpr;
                ce.sourceRectW = srcW * dpr;
                ce.sourceRectH = srcH * dpr;
                ce.glyphCount = tc.glyphEnd - tc.glyphStart;
                ce.glyphStartIndex = glyphBufStart + tc.glyphStart;
                memset(ce.rawFontFingerprint, 0, sizeof(ce.rawFontFingerprint));
                if (rawFontKeyBytes.size() > 0) {
                    int copyLen = rawFontKeyBytes.size();
                    if (copyLen > (int)sizeof(ce.rawFontFingerprint) - 1)
                        copyLen = (int)sizeof(ce.rawFontFingerprint) - 1;
                    memcpy(ce.rawFontFingerprint, rawFontKeyBytes.constData(), copyLen);
                }
                ce.isRTL = run.isRightToLeft();
                ce.firstGlyphIndex = (tc.glyphStart < count)
                    ? g_canonical_cluster_glyph_buf[glyphBufStart + tc.glyphStart].glyphIndex
                    : 0;

                g_canonical_cluster_buf.push_back(ce);
            }
        }

        entry.clusterStartIndex = clusterStartIdx;
        entry.clusterCount = (int)g_canonical_cluster_buf.size() - clusterStartIdx;

        entry.cursorXMapStart = (int)g_cursor_x_map_buf.size();
        entry.cursorXMapCount = 0;
        for (int qpos = entry.qcharStart; qpos <= entry.qcharEnd; qpos++) {
            CursorXMapEntry me;
            me.qcharPos = qpos;
            me.xLeading = line.cursorToX(qpos, QTextLine::Leading) - line.x();
            me.xTrailing = line.cursorToX(qpos, QTextLine::Trailing) - line.x();
            g_cursor_x_map_buf.push_back(me);
            entry.cursorXMapCount++;
        }

        g_canonical_line_buf.push_back(entry);
    }

    void editor_prepare_paragraph_visual_snapshot(
        const QString& paraText,
        double fs, const QString& ff,
        double wrap_w, double indent_w,
        double dpr, const QColor& textColor,
        int cache_slot,
        double line_spacing,
        uint64_t generation,
        bool generate_animation_visuals
    ) {
        g_canonical_line_buf.clear();
        g_canonical_cluster_buf.clear();
        g_canonical_cluster_glyph_buf.clear();
        g_canonical_line_images.clear();
        g_cursor_x_map_buf.clear();

        // Issue #658: 空段落也占一个明确的 null slot，保持与 editor_layout_lines()
        // 对空段落 push_back(nullptr) 的处理一致。这样 cache_slot 与文档段落一一对应，
        // scene_graph_renderer 直接消费 snapshot 中稳定的 slot 不再自行计数。
        // Issue #658 评论 5620035970 问题 2: 用 generation 隔离。
        if (paraText.isEmpty()) {
            if (cache_slot >= 0) {
                set_null_paragraph_layout_slot_gen(generation, cache_slot);
            }
            return;
        }

        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QFontMetricsF fm(font);
        double metrics_h = fm.ascent() + fm.descent();

        // Issue #658 评论 5621512329 问题 2: 用共享排版核心，
        // 与 editor_layout_lines 共用同一份排版逻辑。
        // 从这一个已排好版（含 setPosition）的 layout 同时提取 line/cluster/cursor 数据，
        // 然后存入 g_layout_generations[generation]，供 rebuild_text_node_from_paragraphs() 消费。
        auto core = prepare_paragraph_layout_core(
            paraText, font, wrap_w, indent_w, fs, line_spacing, metrics_h);
        auto* layout = core.layout;
        QVector<QTextLine>& textLines = core.textLines;

        for (int i = 0; i < textLines.size(); i++) {
            const QTextLine& line = textLines[i];
            bool isFirst = (i == 0);

            CanonicalLineEntry entry;
            entry.qcharStart = line.textStart();
            entry.qcharEnd = line.textStart() + line.textLength();
            entry.xPos = isFirst ? indent_w : 0.0;
            entry.width = line.naturalTextWidth();
            entry.height = line.height();
            entry.ascent = line.ascent();
            entry.descent = line.descent();
            entry.y = line.y();
            entry.xEndLeading = line.cursorToX(entry.qcharEnd, QTextLine::Leading) - line.x();
            entry.xEndTrailing = line.cursorToX(entry.qcharEnd, QTextLine::Trailing) - line.x();

            double logical_w = wrap_w;
            double logical_h = line.height();
            int phys_w = (int)ceil(logical_w * dpr);
            int phys_h = (int)ceil(logical_h * dpr);

            // Issue #658 评论 5622829886 问题 2: 分离基础排版与动画视觉生成。
            // generate_animation_visuals=false 时跳过 QImage/glyphRuns/cluster 生成，
            // 只做基础排版（QTextLayout）+ line 几何 + cursor_x_map。
            // 静态 QSGTextNode 直接消费已排好的 QTextLayout，不需要 QImage。
            if (generate_animation_visuals && phys_w > 0 && phys_h > 0 && phys_w <= 8192 && phys_h <= 4096) {
                QImage img(phys_w, phys_h, QImage::Format_ARGB32_Premultiplied);
                img.setDevicePixelRatio(dpr);
                img.fill(Qt::transparent);

                QPainter painter(&img);
                painter.setRenderHint(QPainter::TextAntialiasing, true);
                painter.setPen(QPen(textColor));
                QPointF pos(-line.x(), -line.y());
                line.draw(&painter, pos);

                entry.imagePhysW = phys_w;
                entry.imagePhysH = phys_h;
                g_canonical_line_images.push_back(img);
            } else {
                entry.imagePhysW = 0;
                entry.imagePhysH = 0;
                g_canonical_line_images.push_back(QImage());
            }

            int clusterStartIdx = (int)g_canonical_cluster_buf.size();

            if (generate_animation_visuals) {
                const auto glyphRuns = sujianGlyphRuns(line);

            // Issue #724 评论 5750911834 问题 1: 引入全行 logicalStarts。
            // 收集全行所有 glyph run 的 stringIndexes，用行内真实 cluster 边界
            // 计算 qcharStart/qcharEnd，不再用相邻 cluster 的 qcharVal 猜。
            std::vector<int> lineLogicalStarts;
            for (const auto& run : glyphRuns) {
                const auto& si = run.stringIndexes();
                for (int i = 0; i < (int)si.size(); i++) {
                    if (si[i] >= 0) lineLogicalStarts.push_back(si[i]);
                }
            }
            std::sort(lineLogicalStarts.begin(), lineLogicalStarts.end());
            lineLogicalStarts.erase(
                std::unique(lineLogicalStarts.begin(), lineLogicalStarts.end()),
                lineLogicalStarts.end());

            for (const auto& run : glyphRuns) {
                const auto& positions = run.positions();
                const auto& glyphIndexes = run.glyphIndexes();
                const auto& stringIndexes = run.stringIndexes();
                int count = positions.size();
                if (count == 0) continue;

                QRawFont rawFont = run.rawFont();
                QString rawFontFamily = rawFont.familyName();
                QByteArray rawFontKeyBytes = rawFontFamily.toUtf8();

                int glyphBufStart = (int)g_canonical_cluster_glyph_buf.size();

                for (int gi = 0; gi < count; gi++) {
                    unsigned int gIdx = (gi < glyphIndexes.size()) ? glyphIndexes[gi] : 0;
                    double gx = positions[gi].x();
                    double gy = positions[gi].y();
                    int si = (gi < stringIndexes.size()) ? stringIndexes[gi] : -1;

                    CanonicalClusterGlyphEntry ge;
                    ge.glyphIndex = gIdx;
                    ge.positionX = gx;
                    ge.positionY = gy;
                    ge.stringIndex = si;
                    g_canonical_cluster_glyph_buf.push_back(ge);
                }

                struct TempCluster {
                    int qcharVal;
                    int glyphStart;
                    int glyphEnd;
                    double visMinX, visMinY, visMaxX, visMaxY;
                };
                std::vector<TempCluster> tempClusters;

                if (count > 0) {
                    int curQchar = g_canonical_cluster_glyph_buf[glyphBufStart].stringIndex;
                    int clStart = 0;
                    double clMinX = 1e9, clMinY = 1e9, clMaxX = -1e9, clMaxY = -1e9;

                    for (int gi = 0; gi <= count; gi++) {
                        int si = (gi < count)
                            ? g_canonical_cluster_glyph_buf[glyphBufStart + gi].stringIndex
                            : INT_MAX;

                        if (gi == count || si != curQchar) {
                            if (curQchar >= 0) {
                                TempCluster tc;
                                tc.qcharVal = curQchar;
                                tc.glyphStart = clStart;
                                tc.glyphEnd = gi;
                                tc.visMinX = clMinX;
                                tc.visMinY = clMinY;
                                tc.visMaxX = clMaxX;
                                tc.visMaxY = clMaxY;
                                tempClusters.push_back(tc);
                            }
                            if (gi < count) {
                                curQchar = si;
                                clStart = gi;
                                clMinX = 1e9; clMinY = 1e9;
                                clMaxX = -1e9; clMaxY = -1e9;
                            }
                        }

                        if (gi < count && si == curQchar) {
                            unsigned int gIdx2 = g_canonical_cluster_glyph_buf[glyphBufStart + gi].glyphIndex;
                            double gx2 = g_canonical_cluster_glyph_buf[glyphBufStart + gi].positionX;
                            double gy2 = g_canonical_cluster_glyph_buf[glyphBufStart + gi].positionY;
                            QRectF gb = rawFont.boundingRect(gIdx2);
                            double gl = gx2 + gb.left();
                            double gr = gx2 + gb.right();
                            double gt = gy2 + gb.top();
                            double gbo = gy2 + gb.bottom();
                            if (gl < clMinX) clMinX = gl;
                            if (gr > clMaxX) clMaxX = gr;
                            if (gt < clMinY) clMinY = gt;
                            if (gbo > clMaxY) clMaxY = gbo;
                        }
                    }
                }

                double aaMargin = 1.0;
                for (int ci = 0; ci < (int)tempClusters.size(); ci++) {
                    const TempCluster& tc = tempClusters[ci];
                    if (tc.qcharVal < 0) continue;

                    int qcharStart = tc.qcharVal;
                    int qcharEnd;
                    // Issue #724 评论 5750911834 问题 1: 用全行 logicalStarts 计算真实
                    // cluster 边界，不再用相邻 cluster 的 qcharVal 猜。
                    auto lsIt = std::upper_bound(
                        lineLogicalStarts.begin(), lineLogicalStarts.end(), tc.qcharVal);
                    if (lsIt != lineLogicalStarts.end()) {
                        qcharEnd = *lsIt;
                    } else {
                        qcharEnd = entry.qcharEnd;
                    }
                    if (qcharEnd <= qcharStart) qcharEnd = qcharStart + 1;

                    double srcX = (tc.visMinX - aaMargin) - line.x();
                    double srcY = (tc.visMinY - aaMargin) - line.y();
                    double srcW = (tc.visMaxX - tc.visMinX) + aaMargin * 2.0;
                    double srcH = (tc.visMaxY - tc.visMinY) + aaMargin * 2.0;

                    if (srcW < 0.01) srcW = 10.0;
                    if (srcH < 0.01) srcH = line.height();

                    if (srcX < 0) { srcW += srcX; srcX = 0; }
                    if (srcY < 0) { srcH += srcY; srcY = 0; }
                    if (srcX + srcW > logical_w) srcW = logical_w - srcX;
                    if (srcY + srcH > logical_h) srcH = logical_h - srcY;

                    CanonicalClusterEntry ce;
                    ce.qcharStart = qcharStart;
                    ce.qcharEnd = qcharEnd;
                    ce.sourceRectX = srcX * dpr;
                    ce.sourceRectY = srcY * dpr;
                    ce.sourceRectW = srcW * dpr;
                    ce.sourceRectH = srcH * dpr;
                    ce.glyphCount = tc.glyphEnd - tc.glyphStart;
                    ce.glyphStartIndex = glyphBufStart + tc.glyphStart;
                    memset(ce.rawFontFingerprint, 0, sizeof(ce.rawFontFingerprint));
                    if (rawFontKeyBytes.size() > 0) {
                        int copyLen = rawFontKeyBytes.size();
                        if (copyLen > (int)sizeof(ce.rawFontFingerprint) - 1)
                            copyLen = (int)sizeof(ce.rawFontFingerprint) - 1;
                        memcpy(ce.rawFontFingerprint, rawFontKeyBytes.constData(), copyLen);
                    }
                    ce.isRTL = run.isRightToLeft();
                    ce.firstGlyphIndex = (tc.glyphStart < count)
                        ? g_canonical_cluster_glyph_buf[glyphBufStart + tc.glyphStart].glyphIndex
                        : 0;

                    g_canonical_cluster_buf.push_back(ce);
                }
            }
            } // end if (generate_animation_visuals)

            entry.clusterStartIndex = clusterStartIdx;
            entry.clusterCount = (int)g_canonical_cluster_buf.size() - clusterStartIdx;

            entry.cursorXMapStart = (int)g_cursor_x_map_buf.size();
            entry.cursorXMapCount = 0;
            for (int qpos = entry.qcharStart; qpos <= entry.qcharEnd; qpos++) {
                CursorXMapEntry me;
                me.qcharPos = qpos;
                me.xLeading = line.cursorToX(qpos, QTextLine::Leading) - line.x();
                me.xTrailing = line.cursorToX(qpos, QTextLine::Trailing) - line.x();
                g_cursor_x_map_buf.push_back(me);
                entry.cursorXMapCount++;
            }

            g_canonical_line_buf.push_back(entry);
        }

        // Issue #658: 数据提取完成后，将这一个已排好版（含 setPosition）的 layout
        // 直接存入 g_layout_generations[generation]，供 rebuild_text_node_from_paragraphs() 消费。
        // 不再排第二遍。如果 cache_slot < 0（不缓存），则销毁 layout 避免泄漏。
        // Issue #658 评论 5620035970 问题 2: 用 generation 隔离。
        if (cache_slot >= 0) {
            set_paragraph_layout_slot_gen(generation, cache_slot, layout);
        } else {
            delete layout;
        }
    }

    int editor_canonical_line_count() {
        return static_cast<int>(g_canonical_line_buf.size());
    }

    int editor_canonical_cluster_count() {
        return static_cast<int>(g_canonical_cluster_buf.size());
    }

    int editor_canonical_cluster_glyph_count() {
        return static_cast<int>(g_canonical_cluster_glyph_buf.size());
    }

    void editor_copy_canonical_line_image(int line_idx, QImage* out_img) {
        if (line_idx >= 0 && line_idx < (int)g_canonical_line_images.size()) {
            *out_img = g_canonical_line_images[line_idx];
        }
    }
}}

/// Issue #707 评论 5724685300: 固定 Qt 测试线程。
///
/// Qt 官方线程规则把创建 QCoreApplication/QGuiApplication 的线程视为 GUI/main
/// thread，GUI 相关对象应在该线程使用。`ensure_qt_application()` 只保证
/// QGuiApplication 创建一次，不能保证后续每个 Rust `#[test]` 都在创建
/// QGuiApplication 的同一线程执行。Rust 测试默认并行跑，会违反 Qt 线程规则。
///
/// 本函数建一个专用线程，在该线程内创建 QGuiApplication 并运行通道循环。
/// 所有需要 Qt 的测试逻辑通过 `run_on_qt_thread(|| { ... })` 发到这同一个
/// 线程执行，保证 application 创建和所有 Qt 调用都发生在同一线程。
///
/// 闭包内构造所有 `!Send` 的对象（如 `AppRef`、`SujianEditorItem`），不跨线程
/// 传递。如果闭包 panic，通过 `catch_unwind` 捕获并在调用线程 re-panic，
/// 使测试失败正确传播。
///
/// SAFETY: QGuiApplication 在专用线程创建后不 delete，生命周期与线程相同。
/// offscreen platform 不需要真实显示。所有 Qt 调用都在该线程执行。
#[cfg(any(test, feature = "test-helpers"))]
pub fn run_on_qt_thread<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    use std::sync::mpsc::channel;
    use std::sync::OnceLock;

    struct QtThreadHandle {
        sender: std::sync::mpsc::Sender<Box<dyn FnOnce() + Send + 'static>>,
    }

    static QT_THREAD: OnceLock<QtThreadHandle> = OnceLock::new();

    fn qt_thread() -> &'static QtThreadHandle {
        QT_THREAD.get_or_init(|| {
            let (sender, receiver) = channel::<Box<dyn FnOnce() + Send + 'static>>();
            std::thread::Builder::new()
                .name("qt-test-thread".to_string())
                .spawn(move || {
                    // 在这个专用线程内创建 QGuiApplication。
                    std::env::set_var("QT_QPA_PLATFORM", "offscreen");
                    // SAFETY: QGuiApplication 在这个专用线程创建，生命周期与线程相同。
                    // 所有 Qt 调用都通过 run_on_qt_thread 在这个线程执行。
                    cpp!(unsafe [] {
                        static int argc = 1;
                        static char argv0[] = "sujian-test";
                        static char* argv[] = {argv0, nullptr};
                        static QGuiApplication* app = nullptr;
                        if (QGuiApplication::instance() == nullptr && app == nullptr) {
                            app = new QGuiApplication(argc, argv);
                        }
                    });
                    // 消息循环：接收闭包并执行，panic 不终止线程。
                    while let Ok(f) = receiver.recv() {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
                    }
                })
                .expect("failed to spawn qt test thread");
            QtThreadHandle { sender }
        })
    }

    let (result_tx, result_rx) = channel();
    qt_thread()
        .sender
        .send(Box::new(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            let _ = result_tx.send(result);
        }))
        .expect("qt test thread channel send failed");
    let result = result_rx
        .recv()
        .expect("qt test thread channel recv failed");
    if let Err(panic_payload) = result {
        std::panic::resume_unwind(panic_payload);
    }
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

pub fn get_font_ascent(font_family: &str, font_size: f32) -> f64 {
    let family = QString::from(font_family);
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [family as "QString", font_size as "float"] -> f64 as "double" {
        QFont font(family);
        font.setPixelSize(font_size);
        QFontMetricsF metrics(font);
        return metrics.ascent();
    })
}

pub fn get_font_descent(font_family: &str, font_size: f32) -> f64 {
    let family = QString::from(font_family);
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [family as "QString", font_size as "float"] -> f64 as "double" {
        QFont font(family);
        font.setPixelSize(font_size);
        QFontMetricsF metrics(font);
        return metrics.descent();
    })
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
    let h = (ascent + descent).min(line.height);
    let mut top_y = baseline - ascent;
    if top_y < line.y {
        top_y = line.y;
    }
    if top_y + h > line.y + line.height {
        top_y = line.y + line.height - h;
    }
    (top_y, h)
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

/// Issue #748: 用 QFontMetricsF::horizontalAdvance 测量文本宽度（纯 QFont 测量），
/// 不创建 QTextLayout，符合"正式路径只从 PreparedLayoutHandle/QTextLine 走"的要求。
/// 供 EditorLayout::text_width 测量 preedit/IME 文本宽度。
/// SAFETY: GUI thread only; QFont/QFontMetricsF 不依赖 QTextLayout 生命周期。
pub fn text_width(text: &str, font_size: f64, font_family: &str) -> f64 {
    let qtext: QString = text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    // SAFETY: GUI thread only; QFont/QFontMetricsF 不依赖 QTextLayout 生命周期。
    cpp!(unsafe [qtext as "QString", fs as "float", ff as "QString"] -> f64 as "double" {
        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QFontMetricsF metrics(font);
        return metrics.horizontalAdvance(qtext);
    })
}

pub fn prepare_paragraph_visual_snapshot(
    paragraph_text: &str,
    paragraph_document_byte_start: usize,
    font_size: f64,
    font_family: &str,
    wrap_w: f64,
    indent_w: f64,
    dpr: f64,
    text_color: Option<&str>,
    cache_slot: i32,
    line_spacing: f64,
    generation: u64,
    generate_animation_visuals: bool,
) -> CanonicalParagraphSnapshot {
    let index_map = crate::editor::paragraph_index_map::ParagraphIndexMap::build(
        paragraph_text,
        paragraph_document_byte_start,
    );

    // Issue #688: 静态布局路径不接收颜色参数；只有 generate_animation_visuals=true 时才使用真实颜色
    let color = if generate_animation_visuals {
        text_color.map_or_else(
            || qmetaobject::QColor::from_name(""),
            |c| qmetaobject::QColor::from_name(c),
        )
    } else {
        qmetaobject::QColor::from_name("")
    };

    if paragraph_text.is_empty() {
        // Issue #658: 空段落也调用 C++ 占 null slot，保持 cache_slot 与文档段落一一对应。
        let para: QString = paragraph_text.to_string().into();
        let fs = font_size as f32;
        let ff: QString = font_family.to_string().into();
        let ls = line_spacing;
        // SAFETY: GUI thread only; cache_slot 是文档段落索引，由调用方保证有效。
        cpp!(unsafe [
            para as "QString",
            fs as "float",
            ff as "QString",
            wrap_w as "double",
            indent_w as "double",
            dpr as "double",
            color as "QColor",
            cache_slot as "int",
            ls as "double",
            generation as "uint64_t",
            generate_animation_visuals as "bool"
        ] {
            editor_prepare_paragraph_visual_snapshot(para, fs, ff, wrap_w, indent_w, dpr, color, cache_slot, ls, generation, generate_animation_visuals);
        });
        return CanonicalParagraphSnapshot {
            paragraph_text: paragraph_text.to_string(),
            paragraph_document_byte_start,
            lines: Vec::new(),
            index_map,
        };
    }

    let para: QString = paragraph_text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    let ls = line_spacing;

    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    let line_count = cpp!(unsafe [
        para as "QString",
        fs as "float",
        ff as "QString",
        wrap_w as "double",
        indent_w as "double",
        dpr as "double",
        color as "QColor",
        cache_slot as "int",
        ls as "double",
        generation as "uint64_t",
        generate_animation_visuals as "bool"
    ] -> i32 as "int" {
        editor_prepare_paragraph_visual_snapshot(para, fs, ff, wrap_w, indent_w, dpr, color, cache_slot, ls, generation, generate_animation_visuals);
        return static_cast<int>(g_canonical_line_buf.size());
    });

    let mut lines = Vec::with_capacity(line_count as usize);

    for line_idx in 0..line_count {
        let idx = line_idx;

        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let qchar_start = cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return static_cast<qulonglong>(g_canonical_line_buf[idx].qcharStart);
            return 0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let qchar_end = cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return static_cast<qulonglong>(g_canonical_line_buf[idx].qcharEnd);
            return 0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let x_pos = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].xPos;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let width = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].width;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let _height = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].height;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let ascent = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].ascent;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let descent = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].descent;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let _y = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].y;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let _x_end_leading = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].xEndLeading;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let x_end_trailing = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].xEndTrailing;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let cluster_start = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].clusterStartIndex;
            return 0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let cluster_count = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].clusterCount;
            return 0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let image_phys_w = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].imagePhysW;
            return 0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let image_phys_h = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].imagePhysH;
            return 0;
        });

        let image = if image_phys_w > 0 && image_phys_h > 0 {
            let mut img = qmetaobject::QImage::new(
                qmetaobject::QSize {
                    width: 1,
                    height: 1,
                },
                qmetaobject::ImageFormat::ARGB32_Premultiplied,
            );
            let img_ptr = &mut img as *mut qmetaobject::QImage;
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            cpp!(unsafe [img_ptr as "QImage*", idx as "int"] {
                editor_copy_canonical_line_image(idx, img_ptr);
            });
            Some(img)
        } else {
            None
        };

        let doc_byte_start = index_map.qchar_to_document_byte(qchar_start);
        let doc_byte_end = index_map.qchar_to_document_byte(qchar_end);

        let mut clusters = Vec::with_capacity(cluster_count as usize);
        for ci in 0..cluster_count {
            let cidx = cluster_start + ci;

            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_qchar_start = cpp!(unsafe [cidx as "int"] -> usize as "qulonglong" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return static_cast<qulonglong>(g_canonical_cluster_buf[cidx].qcharStart);
                return 0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_qchar_end = cpp!(unsafe [cidx as "int"] -> usize as "qulonglong" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return static_cast<qulonglong>(g_canonical_cluster_buf[cidx].qcharEnd);
                return 0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_src_x = cpp!(unsafe [cidx as "int"] -> f64 as "double" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].sourceRectX;
                return 0.0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_src_y = cpp!(unsafe [cidx as "int"] -> f64 as "double" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].sourceRectY;
                return 0.0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_src_w = cpp!(unsafe [cidx as "int"] -> f64 as "double" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].sourceRectW;
                return 0.0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_src_h = cpp!(unsafe [cidx as "int"] -> f64 as "double" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].sourceRectH;
                return 0.0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_glyph_count = cpp!(unsafe [cidx as "int"] -> i32 as "int" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].glyphCount;
                return 0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_raw_font: QString = cpp!(unsafe [cidx as "int"] -> QString as "QString" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return QString::fromUtf8(g_canonical_cluster_buf[cidx].rawFontFingerprint);
                return QString();
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_is_rtl = cpp!(unsafe [cidx as "int"] -> bool as "bool" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].isRTL;
                return false;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_first_glyph = cpp!(unsafe [cidx as "int"] -> u32 as "quint32" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].firstGlyphIndex;
                return 0;
            });

            let c_doc_byte_start = index_map.qchar_to_document_byte(c_qchar_start);
            let c_doc_byte_end = index_map.qchar_to_document_byte(c_qchar_end);

            let (c_byte_start, c_byte_end) =
                index_map.qchar_range_to_document_byte_range(c_qchar_start, c_qchar_end);
            let c_cluster_text: String = if c_byte_start <= c_byte_end
                && c_byte_end <= paragraph_text.len() + paragraph_document_byte_start
            {
                let local_start = c_byte_start.saturating_sub(paragraph_document_byte_start);
                let local_end = c_byte_end.saturating_sub(paragraph_document_byte_start);
                if local_start <= local_end && local_end <= paragraph_text.len() {
                    paragraph_text[local_start..local_end].to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            clusters.push(CanonicalClusterSnapshot {
                document_byte_start: c_doc_byte_start,
                document_byte_end: c_doc_byte_end,
                source_rect_x: c_src_x,
                source_rect_y: c_src_y,
                source_rect_w: c_src_w,
                source_rect_h: c_src_h,
                glyph_count: c_glyph_count as usize,
                raw_font_fingerprint: c_raw_font.to_string(),
                is_rtl: c_is_rtl,
                first_glyph_index: c_first_glyph,
                cluster_text: c_cluster_text,
            });
        }

        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let cursor_x_map_start = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].cursorXMapStart;
            return 0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let cursor_x_map_count = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].cursorXMapCount;
            return 0;
        });

        let mut cursor_x_map = Vec::with_capacity(cursor_x_map_count as usize);
        for mi in 0..cursor_x_map_count {
            let midx = cursor_x_map_start + mi;
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let m_qchar = cpp!(unsafe [midx as "int"] -> usize as "qulonglong" {
                if (midx >= 0 && midx < (int)g_cursor_x_map_buf.size())
                    return static_cast<qulonglong>(g_cursor_x_map_buf[midx].qcharPos);
                return 0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let m_x_leading = cpp!(unsafe [midx as "int"] -> f64 as "double" {
                if (midx >= 0 && midx < (int)g_cursor_x_map_buf.size())
                    return g_cursor_x_map_buf[midx].xLeading;
                return 0.0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let m_x_trailing = cpp!(unsafe [midx as "int"] -> f64 as "double" {
                if (midx >= 0 && midx < (int)g_cursor_x_map_buf.size())
                    return g_cursor_x_map_buf[midx].xTrailing;
                return 0.0;
            });
            cursor_x_map.push(CursorXMapEntry {
                qchar_pos: m_qchar,
                x_leading: m_x_leading,
                x_trailing: m_x_trailing,
            });
        }

        lines.push(CanonicalLineSnapshot {
            qchar_start,
            qchar_end,
            document_byte_start: doc_byte_start,
            document_byte_end: doc_byte_end,
            x_pos,
            width,
            ascent,
            descent,
            x_end_trailing,
            image,
            clusters,
            cursor_x_map,
        });
    }

    CanonicalParagraphSnapshot {
        paragraph_text: paragraph_text.to_string(),
        paragraph_document_byte_start,
        lines,
        index_map,
    }
}
