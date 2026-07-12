use cpp::cpp;
use qmetaobject::QString;

cpp! {{
    #include <QtGlobal>
    #include <QtGui/QFont>
    #include <QtGui/QFontMetricsF>
    #include <QtGui/QPainter>
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

    // Glyph entry for glyphRuns-based positioning
    struct GlyphEntry {
        int stringIndex;    // QChar index in the source string
        double xPos;        // Left edge of the glyph
        double width;       // Advance width of the glyph
        unsigned int glyphIndex;  // Real glyph index from QGlyphRun
        char rawFontKey[256];     // Font family name from QRawFont (for font_id)
    };
    thread_local std::vector<GlyphEntry> g_glyph_buf;

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
                if (qEnvironmentVariableIsSet("SUJIAN_EDITOR_DEBUG")) {
                    qDebug("[debug_line_metrics] qtextline=%d exists=1 textStart=%d textLength=%d lineEnd=%d width=%.4f naturalTextWidth=%.4f cursorToX(textStart,Leading)=%.4f cursorToX(lineEnd,Leading)=%.4f cursorToX(lineEnd,Trailing)=%.4f xToCursor(naturalTextWidth)=%d xToCursor(width)=%d",
                        qtextline_idx, line_start, line.textLength(), line_end,
                        line.width(), line.naturalTextWidth(),
                        line.cursorToX(line_start, QTextLine::Leading),
                        line.cursorToX(line_end, QTextLine::Leading),
                        line.cursorToX(line_end, QTextLine::Trailing),
                        line.xToCursor(line.naturalTextWidth()),
                        line.xToCursor(line.width()));
                }
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
                const auto glyphRuns = line.glyphRuns();

#if QT_VERSION >= QT_VERSION_CHECK(6, 5, 0)
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
#else
                // Qt < 6.5: stringIndexes() unavailable, use cursorToX per character
                int line_start = line.textStart();
                int line_end = line_start + line.textLength();
                int range_start = (range_qchar_start > line_start) ? range_qchar_start : line_start;
                int range_end = (range_qchar_end < line_end) ? range_qchar_end : line_end;

                for (int idx = range_start; idx < range_end; idx++) {
                    double x = line.cursorToX(idx, QTextLine::Leading);
                    double x_next = line.cursorToX(idx + 1, QTextLine::Leading);
                    double w = x_next - x;
                    if (w < 0) w = -w; // RTL text

                    GlyphEntry e;
                    e.stringIndex = idx;
                    e.xPos = x;
                    e.width = w;
                    e.glyphIndex = 0;
                    memset(e.rawFontKey, 0, sizeof(e.rawFontKey));
                    g_glyph_buf.push_back(e);
                }
#endif

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
                const auto glyphRuns = line.glyphRuns();
                double lineY = line.y();
                double lineH = line.height();
                double lineAscent = line.ascent();

                int runIdx = 0;
                for (const auto& run : glyphRuns) {
                    const auto& positions = run.positions();
                    const auto& glyphIndexes = run.glyphIndexes();
#if QT_VERSION >= QT_VERSION_CHECK(6, 5, 0)
                    const auto& stringIndexes = run.stringIndexes();
#endif
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
#if QT_VERSION >= QT_VERSION_CHECK(6, 5, 0)
                        int si = (i < stringIndexes.size()) ? stringIndexes[i] : -1;
#else
                        int si = -1;
#endif
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
#if QT_VERSION >= QT_VERSION_CHECK(6, 5, 0)
                        ge.stringIndex = (i < stringIndexes.size()) ? stringIndexes[i] : -1;
#else
                        ge.stringIndex = -1;
#endif
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
    };

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

    void editor_prepare_paragraph_visual_snapshot(
        const QString& paraText,
        double fs, const QString& ff,
        double wrap_w, double indent_w,
        double dpr, const QColor& textColor
    ) {
        g_canonical_line_buf.clear();
        g_canonical_cluster_buf.clear();
        g_canonical_cluster_glyph_buf.clear();
        g_canonical_line_images.clear();

        if (paraText.isEmpty()) return;

        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QTextLayout layout(paraText, font);
        QTextOption option;
        option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
        layout.setTextOption(option);
        layout.setCacheEnabled(true);
        layout.beginLayout();

        QVector<QTextLine> textLines;
        bool first = true;
        while (true) {
            QTextLine line = layout.createLine();
            if (!line.isValid()) break;
            double lineWrap = first ? (wrap_w - indent_w) : wrap_w;
            line.setLineWidth(lineWrap);
            textLines.push_back(line);
            first = false;
        }
        layout.endLayout();

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
            entry.xEndLeading = line.cursorToX(entry.qcharEnd, QTextLine::Leading);
            entry.xEndTrailing = line.cursorToX(entry.qcharEnd, QTextLine::Trailing);

            double logical_w = wrap_w;
            double logical_h = line.height();
            int phys_w = (int)ceil(logical_w * dpr);
            int phys_h = (int)ceil(logical_h * dpr);

            if (phys_w > 0 && phys_h > 0 && phys_w <= 8192 && phys_h <= 4096) {
                QImage img(phys_w, phys_h, QImage::Format_ARGB32_Premultiplied);
                img.setDevicePixelRatio(dpr);
                img.fill(Qt::transparent);

                QPainter painter(&img);
                painter.setRenderHint(QPainter::TextAntialiasing, true);
                painter.scale(dpr, dpr);
                painter.setPen(QPen(textColor));
                QPointF pos(0, line.ascent());
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

            const auto glyphRuns = line.glyphRuns();

            for (const auto& run : glyphRuns) {
                const auto& positions = run.positions();
                const auto& glyphIndexes = run.glyphIndexes();
#if QT_VERSION >= QT_VERSION_CHECK(6, 5, 0)
                const auto& stringIndexes = run.stringIndexes();
#endif
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
#if QT_VERSION >= QT_VERSION_CHECK(6, 5, 0)
                    int si = (gi < stringIndexes.size()) ? stringIndexes[gi] : -1;
#else
                    int si = -1;
#endif

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
                    if (ci + 1 < (int)tempClusters.size()) {
                        qcharEnd = tempClusters[ci + 1].qcharVal;
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

            g_canonical_line_buf.push_back(entry);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaretAffinity {
    Upstream,
    Downstream,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VisualLine {
    pub id: usize,
    /// Byte offset of the line start in the full document text (UTF-8).
    pub byte_start: usize,
    /// Byte offset of the line end in the full document text (UTF-8).
    pub byte_end: usize,
    /// QChar (UTF-16 code unit) offset of the line start in the full document text.
    pub qchar_start: usize,
    /// QChar (UTF-16 code unit) offset of the line end in the full document text.
    pub qchar_end: usize,
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
    pub text_revision: u64,
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

    pub fn snapshot(
        &mut self,
        text: &str,
        params: LayoutParams,
        text_revision: u64,
    ) -> &LayoutSnapshot {
        let text_ptr = text.as_ptr() as usize;
        let text_len = text.len();
        let needs_refresh = match &self.cache {
            Some(c) => {
                c.text_revision != text_revision
                    || c.text_ptr != text_ptr
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
            self.cache = None;
        }

        self.cache.get_or_insert_with(|| {
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
            LayoutSnapshot {
                text_revision,
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
            }
        })
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
    ) -> Vec<(usize, f64, f64, u32, String)> {
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
    let metrics_h = get_font_ascent(font_family, font_size as f32)
        + get_font_descent(font_family, font_size as f32);
    let line_height = (font_size * line_spacing)
        .max(font_size + 4.0)
        .max(metrics_h);
    let available = (width - padding * 2.0).max(font_size);
    let mut result = Vec::new();
    let mut y = padding;
    let mut paragraph_start = 0;
    let mut paragraph_qchar_start = 0;
    let mut line_id: usize = 0;

    for paragraph in text.split_inclusive('\n') {
        let hard_break = paragraph.ends_with('\n');
        let paragraph_text = paragraph.trim_end_matches('\n');
        let paragraph_text_end = paragraph_start + paragraph_text.len();

        if paragraph_text.is_empty() {
            let empty_ascent = get_font_ascent(font_family, font_size as f32);
            let empty_descent = get_font_descent(font_family, font_size as f32);
            result.push(VisualLine {
                id: line_id,
                byte_start: paragraph_start,
                byte_end: paragraph_start,
                qchar_start: paragraph_qchar_start,
                qchar_end: paragraph_qchar_start,
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
                qt_ascent: empty_ascent,
                qt_descent: empty_descent,
            });
            line_id += 1;
            y += line_height;
            paragraph_start += paragraph.len();
            paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();
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
                byte_start: abs_start,
                byte_end: abs_end,
                qchar_start: paragraph_qchar_start + qchar_off,
                qchar_end: paragraph_qchar_start + qchar_end,
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
        paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();
    }

    if text.ends_with('\n') {
        let text_qchar_len: usize = text.chars().map(|c| c.len_utf16()).sum();
        result.push(VisualLine {
            id: line_id,
            byte_start: text.len(),
            byte_end: text.len(),
            qchar_start: text_qchar_len,
            qchar_end: text_qchar_len,
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
            byte_start: 0,
            byte_end: 0,
            qchar_start: 0,
            qchar_end: 0,
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
            let Some(l) = lines.last() else {
                return (0, CaretAffinity::Downstream);
            };
            (lines.len() - 1, l)
        }
    };
    let raw_index = index_at_line_x(snapshot, line, x);
    let index = raw_index.max(line.byte_start).min(line.byte_end);
    debug_assert!(
        index >= line.byte_start && index <= line.byte_end,
        "hit_test: index {} out of line range {}..{}",
        index,
        line.byte_start,
        line.byte_end
    );
    let affinity = affinity_for_index_on_line(line, index);

    if std::env::var("SUJIAN_EDITOR_DEBUG").is_ok() {
        eprintln!(
            "[hit_test] cursor={}, affinity={:?}, hit_visual_line_id={}, line.byte_start={}, line.byte_end={}, line.x={:.1}, line.y={:.1}, line.width={:.1}, line.para_start={}, line.qtextline_idx={}, line.para_qchar_start={}, line.para_qchar_end={}, line.line_wrap_width={:.1}, line.line_indent_x={:.1}, line.para_indent={:.1}, x_end_trailing={:.1}",
            index, affinity, line.id, line.byte_start, line.byte_end, line.x, line.y, line.width,
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
                byte_start: 0,
                byte_end: 0,
                qchar_start: 0,
                qchar_end: 0,
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
        return line.byte_start;
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

pub fn calculate_cursor_x_for_line(
    line: &VisualLine,
    cursor: usize,
    affinity: CaretAffinity,
    snapshot: &LayoutSnapshot,
) -> f64 {
    if line.para_text.is_empty() {
        if line.width > 0.0 && cursor == line.byte_end {
            line.x + line.width
        } else {
            line.x
        }
    } else {
        // Always use real-time QTextLine::cursorToX() for cursor position.
        // The cached x_end_trailing is only used as a fallback when the
        // real-time calculation fails (returns 0 for non-empty lines).
        let use_trailing = affinity == CaretAffinity::Upstream && cursor == line.byte_end;
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

        // Fallback: if real-time cursorToX returns near-zero for a non-empty
        // line, use the cached x_end_trailing as a last resort.
        if x <= line.x + 0.5
            && line.byte_start != line.byte_end
            && affinity == CaretAffinity::Upstream
            && cursor == line.byte_end
            && line.x_end_trailing > 0.0
        {
            let fallback_x = line.x + line.x_end_trailing;
            crate::sujian_editor_item::editor_debug_log(&format!(
                    "[calculate_cursor_x] fallback to cached x_end_trailing: cursor={}, line.byte_end={}, x_end_trailing={:.4}, realtime_x={:.4}, fallback_x={:.4}",
                    cursor, line.byte_end, line.x_end_trailing, x, fallback_x
                ));
            return fallback_x;
        }

        if x <= 1.0
            && line.byte_start != line.byte_end
            && std::env::var("SUJIAN_EDITOR_DEBUG").is_ok()
        {
            let cursor_in_para = cursor.saturating_sub(line.para_start);
            let cursor_qchar = byte_offset_to_qchar_offset(&line.para_text, cursor_in_para);
            let line_end_byte_in_para = line.byte_end.saturating_sub(line.para_start);
            let line_end_qchar =
                byte_offset_to_qchar_offset(&line.para_text, line_end_byte_in_para);
            eprintln!(
                "[INVARIANT] cursor_x <= 1.0 for non-empty line!\n\
                 VisualLine: para_qchar_start={}, para_qchar_end={}, start={}, end={}\n\
                 Qt helper: textStart=para_qchar_start ({}), lineEnd={} (from qcharEnd)\n\
                 input cursor_qchar={}, cursor_abs_byte={}, qtextline_idx={}\n\
                 line.byte_end byte -> qchar offset={}\n\
                 Qt cursorToX result={:.4}, line.x={:.4}",
                line.para_qchar_start,
                line.para_qchar_end,
                line.byte_start,
                line.byte_end,
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

#[allow(dead_code)]
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
) -> Vec<(usize, f64, f64, u32, String)> {
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
        return static_cast<int>(g_glyph_buf.size());
    });
    let mut result = Vec::with_capacity(count as usize);
    for i in 0..count {
        let idx = i;
        let qchar_off = cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
            return static_cast<qulonglong>(g_glyph_buf[idx].stringIndex);
        });
        let w = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_glyph_buf[idx].width;
        });
        let x_pos = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_glyph_buf[idx].xPos;
        });
        let glyph_idx = cpp!(unsafe [idx as "int"] -> u32 as "quint32" {
            return g_glyph_buf[idx].glyphIndex;
        });
        let raw_font_family = cpp!(unsafe [idx as "int"] -> QString as "QString" {
            return QString::fromUtf8(g_glyph_buf[idx].rawFontKey);
        });
        let raw_font_family_str = raw_font_family.to_string();
        let para_byte = qchar_offset_to_byte_offset(para_text, qchar_off);
        let abs_byte = para_start + para_byte;
        result.push((abs_byte, x_pos, w, glyph_idx, raw_font_family_str));
    }
    result
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct ShapedRunData {
    pub run_index: i32,
    pub glyph_count: usize,
    pub string_start: usize,
    pub string_end: usize,
    pub is_rtl: bool,
    pub has_underline: bool,
    pub raw_font_family: String,
    pub raw_font_style: String,
    pub raw_font_weight: i32,
    pub raw_font_pixel_size: i32,
    pub baseline_y: f64,
    pub visual_x: f64,
    pub visual_y: f64,
    pub visual_w: f64,
    pub visual_h: f64,
    pub texture_translate_x: f64,
    pub texture_translate_y: f64,
    pub line_y: f64,
    pub glyphs: Vec<RunGlyphData>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct RunGlyphData {
    pub glyph_index: u32,
    pub position_x: f64,
    pub position_y: f64,
    pub string_index: i32,
    pub advance_width: f64,
}

#[allow(dead_code)]
pub fn extract_shaped_runs_on_line(
    para_text: &str,
    range_start: usize,
    range_end: usize,
    para_start: usize,
    font_size: f64,
    font_family: &str,
    paragraph_wrap_w: f64,
    indent_w: f64,
    qtextline_idx: i32,
) -> Vec<ShapedRunData> {
    let seg_start_in_para = range_start.saturating_sub(para_start);
    let seg_end_in_para = range_end.saturating_sub(para_start).min(para_text.len());
    let qchar_start = byte_offset_to_qchar_offset(para_text, seg_start_in_para) as i32;
    let qchar_end = byte_offset_to_qchar_offset(para_text, seg_end_in_para) as i32;
    let para: QString = para_text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    let run_count = cpp!(unsafe [
        para as "QString",
        qchar_start as "int",
        qchar_end as "int",
        fs as "float",
        ff as "QString",
        paragraph_wrap_w as "double",
        indent_w as "double",
        qtextline_idx as "int"
    ] -> i32 as "int" {
        editor_layout_shaped_runs_on_line(
            para, qchar_start, qchar_end, fs, ff, paragraph_wrap_w, indent_w, qtextline_idx
        );
        return static_cast<int>(g_shaped_run_buf.size());
    });

    let mut result = Vec::with_capacity(run_count as usize);
    for i in 0..run_count {
        let idx = i;
        let run_index = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            return g_shaped_run_buf[idx].runIndex;
        });
        let glyph_count = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            return g_shaped_run_buf[idx].glyphCount;
        });
        let str_start = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            return g_shaped_run_buf[idx].stringStart;
        });
        let str_end = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            return g_shaped_run_buf[idx].stringEnd;
        });
        let is_rtl = cpp!(unsafe [idx as "int"] -> bool as "bool" {
            return g_shaped_run_buf[idx].isRTL;
        });
        let has_underline = cpp!(unsafe [idx as "int"] -> bool as "bool" {
            return g_shaped_run_buf[idx].hasUnderline;
        });
        let raw_font_family = cpp!(unsafe [idx as "int"] -> QString as "QString" {
            return QString::fromUtf8(g_shaped_run_buf[idx].rawFontFamily);
        });
        let raw_font_style = cpp!(unsafe [idx as "int"] -> QString as "QString" {
            return QString::fromUtf8(g_shaped_run_buf[idx].rawFontStyle);
        });
        let raw_font_weight = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            return g_shaped_run_buf[idx].rawFontWeight;
        });
        let raw_font_pixel_size = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            return g_shaped_run_buf[idx].rawFontPixelSize;
        });
        let baseline_y = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_shaped_run_buf[idx].baselineY;
        });
        let visual_x = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_shaped_run_buf[idx].visualX;
        });
        let visual_y = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_shaped_run_buf[idx].visualY;
        });
        let visual_w = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_shaped_run_buf[idx].visualW;
        });
        let visual_h = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_shaped_run_buf[idx].visualH;
        });
        let texture_translate_x = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_shaped_run_buf[idx].textureTranslateX;
        });
        let texture_translate_y = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_shaped_run_buf[idx].textureTranslateY;
        });
        let line_y = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_shaped_run_buf[idx].lineY;
        });

        let mut glyphs = Vec::with_capacity(glyph_count as usize);
        let glyph_offset: i32 = (0..i).map(|prev_idx| {
            let prev_count = cpp!(unsafe [prev_idx as "int"] -> i32 as "int" {
                return g_shaped_run_buf[prev_idx].glyphCount;
            });
            prev_count
        }).sum();
        for gi in 0..glyph_count {
            let glyph_idx_in_buf = glyph_offset + gi;
            let g_glyph_index = cpp!(unsafe [glyph_idx_in_buf as "int"] -> u32 as "quint32" {
                if (glyph_idx_in_buf >= 0 && glyph_idx_in_buf < (int)g_run_glyph_buf.size())
                    return g_run_glyph_buf[glyph_idx_in_buf].glyphIndex;
                return 0u;
            });
            let g_pos_x = cpp!(unsafe [glyph_idx_in_buf as "int"] -> f64 as "double" {
                if (glyph_idx_in_buf >= 0 && glyph_idx_in_buf < (int)g_run_glyph_buf.size())
                    return g_run_glyph_buf[glyph_idx_in_buf].positionX;
                return 0.0;
            });
            let g_pos_y = cpp!(unsafe [glyph_idx_in_buf as "int"] -> f64 as "double" {
                if (glyph_idx_in_buf >= 0 && glyph_idx_in_buf < (int)g_run_glyph_buf.size())
                    return g_run_glyph_buf[glyph_idx_in_buf].positionY;
                return 0.0;
            });
            let g_string_index = cpp!(unsafe [glyph_idx_in_buf as "int"] -> i32 as "int" {
                if (glyph_idx_in_buf >= 0 && glyph_idx_in_buf < (int)g_run_glyph_buf.size())
                    return g_run_glyph_buf[glyph_idx_in_buf].stringIndex;
                return -1;
            });
            let g_advance = cpp!(unsafe [glyph_idx_in_buf as "int"] -> f64 as "double" {
                if (glyph_idx_in_buf >= 0 && glyph_idx_in_buf < (int)g_run_glyph_buf.size())
                    return g_run_glyph_buf[glyph_idx_in_buf].advanceWidth;
                return 0.0;
            });
            glyphs.push(RunGlyphData {
                glyph_index: g_glyph_index,
                position_x: g_pos_x,
                position_y: g_pos_y,
                string_index: g_string_index,
                advance_width: g_advance,
            });
        }

        let str_start_byte = para_start + qchar_offset_to_byte_offset(para_text, str_start as usize);
        let str_end_byte = para_start + qchar_offset_to_byte_offset(para_text, str_end as usize);

        result.push(ShapedRunData {
            run_index,
            glyph_count: glyph_count as usize,
            string_start: str_start_byte,
            string_end: str_end_byte,
            is_rtl,
            has_underline,
            raw_font_family: raw_font_family.to_string(),
            raw_font_style: raw_font_style.to_string(),
            raw_font_weight,
            raw_font_pixel_size,
            baseline_y,
            visual_x,
            visual_y,
            visual_w,
            visual_h,
            texture_translate_x,
            texture_translate_y,
            line_y,
            glyphs,
        });
    }
    result
}

/// Render a single QTextLine to a QImage using QTextLine::draw().
///
/// Qt mature route: this produces a line-level visual snapshot that can be
/// UV-clipped to extract individual glyph runs, clusters, or text segments
/// for animation — without re-laying out text per run. The line snapshot
/// is rendered once; animation textures are extracted via UV coordinates.
///
/// The QImage is rendered at DPR scale. The logical line content starts at
/// (0, 0) in the QImage's logical coordinate space, with the baseline at
/// y = line.ascent().
#[allow(dead_code)]
pub fn render_line_to_image(
    para_text: &str,
    font_size: f64,
    font_family: &str,
    paragraph_wrap_w: f64,
    indent_w: f64,
    qtextline_idx: i32,
    dpr: f64,
    text_color: &str,
) -> Option<qmetaobject::QImage> {
    let logical_w = paragraph_wrap_w;
    let logical_h = font_size * 2.0;
    let phys_w = (logical_w * dpr).ceil() as u32;
    let phys_h = (logical_h * dpr).ceil() as u32;
    if phys_w == 0 || phys_h == 0 || phys_w > 8192 || phys_h > 4096 {
        return None;
    }

    let mut image = qmetaobject::QImage::new(
        qmetaobject::QSize { width: phys_w, height: phys_h },
        qmetaobject::ImageFormat::ARGB32_Premultiplied,
    );
    {
        let img_ptr = &mut image as *mut qmetaobject::QImage;
        cpp!(unsafe [img_ptr as "QImage*"] {
            img_ptr->setDevicePixelRatio(1.0);
        });
    }
    image.fill(qmetaobject::QColor::from_rgba(0, 0, 0, 0));

    let para: QString = para_text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    let color = qmetaobject::QColor::from_name(text_color);

    let img_ptr = &mut image as *mut qmetaobject::QImage;
    cpp!(unsafe [
        img_ptr as "QImage*",
        para as "QString",
        fs as "float",
        ff as "QString",
        paragraph_wrap_w as "double",
        indent_w as "double",
        qtextline_idx as "int",
        dpr as "double",
        color as "QColor"
    ] {
        editor_render_line_to_image(
            img_ptr, para, fs, ff, paragraph_wrap_w, indent_w, qtextline_idx,
            dpr, color
        );
    });

    Some(image)
}

/// This ensures the text rendering uses the same shaping data as
/// cursorToX() / xToCursor(), fixing mixed-script cursor issues.
pub fn draw_line_text(
    painter: &mut qmetaobject::QPainter,
    para_text: &str,
    font_size: f64,
    font_family: &str,
    paragraph_wrap_w: f64,
    indent_w: f64,
    qtextline_idx: i32,
    x: f64,
    baseline_y: f64,
    text_color: &str,
) {
    let para: QString = para_text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    let color = qmetaobject::QColor::from_name(text_color);
    cpp!(unsafe [
        painter as "QPainter*",
        para as "QString",
        fs as "float",
        ff as "QString",
        paragraph_wrap_w as "double",
        indent_w as "double",
        qtextline_idx as "int",
        x as "double",
        baseline_y as "double",
        color as "QColor"
    ] {
        editor_draw_line_text(
            painter, para, fs, ff, paragraph_wrap_w, indent_w, qtextline_idx,
            x, baseline_y, color
        );
    });
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

// ── Canonical paragraph visual snapshot ──

#[derive(Clone, Debug)]
pub struct CanonicalClusterSnapshot {
    pub qchar_start: usize,
    pub qchar_end: usize,
    pub document_byte_start: usize,
    pub document_byte_end: usize,
    pub source_rect_x: f64,
    pub source_rect_y: f64,
    pub source_rect_w: f64,
    pub source_rect_h: f64,
    pub glyph_count: usize,
    pub raw_font_fingerprint: String,
    pub is_rtl: bool,
    pub first_glyph_index: u32,
}

#[derive(Clone)]
pub struct CanonicalLineSnapshot {
    pub qchar_start: usize,
    pub qchar_end: usize,
    pub document_byte_start: usize,
    pub document_byte_end: usize,
    pub x_pos: f64,
    pub width: f64,
    pub height: f64,
    pub ascent: f64,
    pub descent: f64,
    pub y: f64,
    pub x_end_leading: f64,
    pub x_end_trailing: f64,
    pub image: Option<qmetaobject::QImage>,
    pub clusters: Vec<CanonicalClusterSnapshot>,
}

#[derive(Clone)]
pub struct CanonicalParagraphSnapshot {
    pub paragraph_text: String,
    pub paragraph_document_byte_start: usize,
    pub lines: Vec<CanonicalLineSnapshot>,
    pub index_map: crate::editor::paragraph_index_map::ParagraphIndexMap,
}

pub fn prepare_paragraph_visual_snapshot(
    paragraph_text: &str,
    paragraph_document_byte_start: usize,
    font_size: f64,
    font_family: &str,
    wrap_w: f64,
    indent_w: f64,
    dpr: f64,
    text_color: &str,
) -> CanonicalParagraphSnapshot {
    let index_map = crate::editor::paragraph_index_map::ParagraphIndexMap::build(
        paragraph_text,
        paragraph_document_byte_start,
    );

    if paragraph_text.is_empty() {
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
    let color = qmetaobject::QColor::from_name(text_color);

    let line_count = cpp!(unsafe [
        para as "QString",
        fs as "float",
        ff as "QString",
        wrap_w as "double",
        indent_w as "double",
        dpr as "double",
        color as "QColor"
    ] -> i32 as "int" {
        editor_prepare_paragraph_visual_snapshot(para, fs, ff, wrap_w, indent_w, dpr, color);
        return static_cast<int>(g_canonical_line_buf.size());
    });

    let mut lines = Vec::with_capacity(line_count as usize);

    for line_idx in 0..line_count {
        let idx = line_idx;

        let qchar_start = cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return static_cast<qulonglong>(g_canonical_line_buf[idx].qcharStart);
            return 0;
        });
        let qchar_end = cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return static_cast<qulonglong>(g_canonical_line_buf[idx].qcharEnd);
            return 0;
        });
        let x_pos = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].xPos;
            return 0.0;
        });
        let width = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].width;
            return 0.0;
        });
        let height = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].height;
            return 0.0;
        });
        let ascent = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].ascent;
            return 0.0;
        });
        let descent = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].descent;
            return 0.0;
        });
        let y = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].y;
            return 0.0;
        });
        let x_end_leading = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].xEndLeading;
            return 0.0;
        });
        let x_end_trailing = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].xEndTrailing;
            return 0.0;
        });
        let cluster_start = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].clusterStartIndex;
            return 0;
        });
        let cluster_count = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].clusterCount;
            return 0;
        });
        let image_phys_w = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].imagePhysW;
            return 0;
        });
        let image_phys_h = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].imagePhysH;
            return 0;
        });

        let image = if image_phys_w > 0 && image_phys_h > 0 {
            let mut img = qmetaobject::QImage::new(
                qmetaobject::QSize { width: 1, height: 1 },
                qmetaobject::ImageFormat::ARGB32_Premultiplied,
            );
            let img_ptr = &mut img as *mut qmetaobject::QImage;
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

            let c_qchar_start = cpp!(unsafe [cidx as "int"] -> usize as "qulonglong" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return static_cast<qulonglong>(g_canonical_cluster_buf[cidx].qcharStart);
                return 0;
            });
            let c_qchar_end = cpp!(unsafe [cidx as "int"] -> usize as "qulonglong" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return static_cast<qulonglong>(g_canonical_cluster_buf[cidx].qcharEnd);
                return 0;
            });
            let c_src_x = cpp!(unsafe [cidx as "int"] -> f64 as "double" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].sourceRectX;
                return 0.0;
            });
            let c_src_y = cpp!(unsafe [cidx as "int"] -> f64 as "double" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].sourceRectY;
                return 0.0;
            });
            let c_src_w = cpp!(unsafe [cidx as "int"] -> f64 as "double" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].sourceRectW;
                return 0.0;
            });
            let c_src_h = cpp!(unsafe [cidx as "int"] -> f64 as "double" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].sourceRectH;
                return 0.0;
            });
            let c_glyph_count = cpp!(unsafe [cidx as "int"] -> i32 as "int" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].glyphCount;
                return 0;
            });
            let c_raw_font: QString = cpp!(unsafe [cidx as "int"] -> QString as "QString" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return QString::fromUtf8(g_canonical_cluster_buf[cidx].rawFontFingerprint);
                return QString();
            });
            let c_is_rtl = cpp!(unsafe [cidx as "int"] -> bool as "bool" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].isRTL;
                return false;
            });
            let c_first_glyph = cpp!(unsafe [cidx as "int"] -> u32 as "quint32" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].firstGlyphIndex;
                return 0;
            });

            let c_doc_byte_start = index_map.qchar_to_document_byte(c_qchar_start);
            let c_doc_byte_end = index_map.qchar_to_document_byte(c_qchar_end);

            clusters.push(CanonicalClusterSnapshot {
                qchar_start: c_qchar_start,
                qchar_end: c_qchar_end,
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
            });
        }

        lines.push(CanonicalLineSnapshot {
            qchar_start,
            qchar_end,
            document_byte_start: doc_byte_start,
            document_byte_end: doc_byte_end,
            x_pos,
            width,
            height,
            ascent,
            descent,
            y,
            x_end_leading,
            x_end_trailing,
            image,
            clusters,
        });
    }

    CanonicalParagraphSnapshot {
        paragraph_text: paragraph_text.to_string(),
        paragraph_document_byte_start,
        lines,
        index_map,
    }
}

#[derive(Clone)]
pub struct CanonicalDocumentVisualSnapshot {
    pub text_revision: u64,
    pub font_size: f64,
    pub font_family: String,
    pub line_spacing: f64,
    pub text_indent: f64,
    pub padding: f64,
    pub width: f64,
    pub dpr: f64,
    pub text_color: String,
    pub paragraphs: Vec<CanonicalParagraphSnapshot>,
    pub visual_lines: Vec<VisualLine>,
    pub content_height: f64,
}

impl CanonicalDocumentVisualSnapshot {
    pub fn cursor_rect(
        &self,
        cursor_byte: usize,
        affinity: CaretAffinity,
        scroll_y: f64,
        viewport_h: f64,
    ) -> CaretRect {
        let line = self.visual_lines
            .iter()
            .enumerate()
            .find(|(idx, _)| {
                line_contains_cursor_with_affinity(&self.visual_lines, *idx, cursor_byte, affinity)
            })
            .map(|(_, line)| line)
            .or_else(|| self.visual_lines.last());

        let fallback;
        let line = match line {
            Some(line) => line,
            None => {
                fallback = VisualLine {
                    id: 0,
                    byte_start: 0,
                    byte_end: 0,
                    qchar_start: 0,
                    qchar_end: 0,
                    hard_break: true,
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: self.font_size * self.line_spacing,
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

        let cursor_x = self.calculate_cursor_x_for_line(line, cursor_byte, affinity);
        let (cursor_y_doc, cursor_h) = cursor_rect_for_line(line, self.font_size, &self.font_family);
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

    fn calculate_cursor_x_for_line(
        &self,
        line: &VisualLine,
        cursor: usize,
        affinity: CaretAffinity,
    ) -> f64 {
        if line.para_text.is_empty() {
            if line.width > 0.0 && cursor == line.byte_end {
                line.x + line.width
            } else {
                line.x
            }
        } else {
            let use_trailing = affinity == CaretAffinity::Upstream && cursor == line.byte_end;
            let paragraph_wrap_w = line.line_wrap_width + line.line_indent_x;
            let x = line.x
                + qtextlayout_cursor_to_x_on_line(
                    &line.para_text,
                    cursor,
                    line.para_start,
                    self.font_size,
                    &self.font_family,
                    paragraph_wrap_w,
                    line.para_indent,
                    line.qtextline_idx,
                    use_trailing,
                );

            if x <= line.x + 0.5
                && line.byte_start != line.byte_end
                && affinity == CaretAffinity::Upstream
                && cursor == line.byte_end
                && line.x_end_trailing > 0.0
            {
                return line.x + line.x_end_trailing;
            }

            x
        }
    }

    pub fn to_layout_snapshot(&self) -> LayoutSnapshot {
        LayoutSnapshot {
            text_revision: self.text_revision,
            text_ptr: 0,
            text_len: 0,
            width: self.width,
            font_size: self.font_size as f32,
            font_family: self.font_family.clone(),
            line_spacing: self.line_spacing as f32,
            text_indent: self.text_indent as f32,
            padding: self.padding as f32,
            lines: self.visual_lines.clone(),
            content_height: self.content_height as f32,
        }
    }
}

pub fn prepare_document_visual_snapshot(
    text: &str,
    text_revision: u64,
    font_size: f64,
    font_family: &str,
    line_spacing: f64,
    padding: f64,
    indent: f64,
    width: f64,
    dpr: f64,
    text_color: &str,
) -> CanonicalDocumentVisualSnapshot {
    let metrics_h = get_font_ascent(font_family, font_size as f32)
        + get_font_descent(font_family, font_size as f32);
    let line_height = (font_size * line_spacing)
        .max(font_size + 4.0)
        .max(metrics_h);
    let available = (width - padding * 2.0).max(font_size);

    let mut paragraphs = Vec::new();
    let mut visual_lines = Vec::new();
    let mut y: f64 = padding;
    let mut paragraph_start: usize = 0;
    let mut paragraph_qchar_start: usize = 0;
    let mut line_id: usize = 0;

    for paragraph in text.split_inclusive('\n') {
        let hard_break = paragraph.ends_with('\n');
        let paragraph_text = paragraph.trim_end_matches('\n');

        if paragraph_text.is_empty() {
            let empty_ascent = get_font_ascent(font_family, font_size as f32);
            let empty_descent = get_font_descent(font_family, font_size as f32);
            visual_lines.push(VisualLine {
                id: line_id,
                byte_start: paragraph_start,
                byte_end: paragraph_start,
                qchar_start: paragraph_qchar_start,
                qchar_end: paragraph_qchar_start,
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
                qt_ascent: empty_ascent,
                qt_descent: empty_descent,
            });
            line_id += 1;
            y += line_height;
            paragraph_start += paragraph.len();
            paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();

            paragraphs.push(CanonicalParagraphSnapshot {
                paragraph_text: String::new(),
                paragraph_document_byte_start: paragraph_start,
                lines: Vec::new(),
                index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build("", paragraph_start),
            });
            continue;
        }

        let canonical = prepare_paragraph_visual_snapshot(
            paragraph_text,
            paragraph_start,
            font_size,
            font_family,
            available,
            indent,
            dpr,
            text_color,
        );

        for (line_idx, canonical_line) in canonical.lines.iter().enumerate() {
            let qt_metrics_h = canonical_line.ascent + canonical_line.descent;
            let actual_line_h = if qt_metrics_h > 0.0 {
                line_height.max(qt_metrics_h)
            } else {
                line_height
            };

            let is_first = line_idx == 0;

            visual_lines.push(VisualLine {
                id: line_id,
                byte_start: canonical_line.document_byte_start,
                byte_end: canonical_line.document_byte_end,
                qchar_start: canonical_line.qchar_start + paragraph_qchar_start,
                qchar_end: canonical_line.qchar_end + paragraph_qchar_start,
                hard_break: hard_break && line_idx == canonical.lines.len() - 1,
                x: padding + canonical_line.x_pos,
                y,
                width: canonical_line.width,
                height: actual_line_h,
                para_text: paragraph_text.to_string(),
                para_start: paragraph_start,
                qtextline_idx: line_idx as i32,
                para_qchar_start: canonical_line.qchar_start,
                para_qchar_end: canonical_line.qchar_end,
                line_wrap_width: if is_first { available - indent } else { available },
                line_indent_x: if is_first { indent } else { 0.0 },
                para_indent: indent,
                x_end_trailing: canonical_line.x_end_trailing,
                qt_ascent: canonical_line.ascent,
                qt_descent: canonical_line.descent,
            });
            line_id += 1;
            y += actual_line_h;
        }

        paragraph_start += paragraph.len();
        paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();

        paragraphs.push(canonical);
    }

    if text.ends_with('\n') {
        let text_qchar_len: usize = text.chars().map(|c| c.len_utf16()).sum();
        visual_lines.push(VisualLine {
            id: line_id,
            byte_start: text.len(),
            byte_end: text.len(),
            qchar_start: text_qchar_len,
            qchar_end: text_qchar_len,
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
        y += line_height;
    }

    let content_height = y.max(1.0);

    CanonicalDocumentVisualSnapshot {
        text_revision,
        font_size,
        font_family: font_family.to_string(),
        line_spacing,
        text_indent: indent,
        padding,
        width,
        dpr,
        text_color: text_color.to_string(),
        paragraphs,
        visual_lines,
        content_height,
    }
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

/// Determine the caret affinity for a given byte index on a visual line.
///
/// At a wrap boundary (where the cursor sits at the end of one line and the
/// start of the next), we use QTextLine::cursorToX with Leading vs Trailing
/// to decide: if the two positions differ, the cursor is at a wrap boundary
/// and should use Upstream affinity so it renders at the end of the current
/// line rather than the start of the next.
pub fn affinity_for_index_on_line(line: &VisualLine, index: usize) -> CaretAffinity {
    if line.byte_start == line.byte_end || line.para_text.is_empty() {
        return CaretAffinity::Downstream;
    }
    // Only the line-end position can be a wrap boundary
    if index != line.byte_end {
        return CaretAffinity::Downstream;
    }
    // Convert byte index to qchar index within the paragraph.
    let cursor_in_para = index.saturating_sub(line.para_start);
    let cursor_qchar = byte_offset_to_qchar_offset(&line.para_text, cursor_in_para);
    // If the qchar index equals para_qchar_end, the cursor is at the line end
    // in qchar space, which means it's a wrap boundary candidate.
    if cursor_qchar == line.para_qchar_end && line.para_qchar_start != line.para_qchar_end {
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
    if line.byte_start == line.byte_end {
        return cursor == line.byte_start;
    }
    if cursor > line.byte_start && cursor < line.byte_end {
        return true;
    }
    if cursor == line.byte_start {
        let has_prev_overlap = idx > 0 && lines[idx - 1].byte_end == line.byte_start;
        if has_prev_overlap {
            return affinity == CaretAffinity::Downstream;
        }
        return true;
    }
    if cursor == line.byte_end {
        let has_next_overlap = idx + 1 < lines.len() && lines[idx + 1].byte_start == line.byte_end;
        if has_next_overlap {
            return affinity == CaretAffinity::Upstream;
        }
        return true;
    }
    false
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
        layout.snapshot(text, params(width), 1).clone()
    }

    fn assert_line_end_roundtrip(snapshot: &LayoutSnapshot) {
        for line in &snapshot.lines {
            if line.byte_start == line.byte_end || line.para_text.is_empty() {
                continue;
            }
            let x = qtextlayout_cursor_to_x_on_line(
                &line.para_text,
                line.byte_end,
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
                line.byte_start,
                line.byte_end
            );
            let rect = caret_rect(snapshot, line.byte_end, CaretAffinity::Upstream, 0.0, 800.0);
            assert_eq!(
                rect.visual_line_id, line.id,
                "caret_rect(line.byte_end, Upstream) must stay on the source visual line"
            );
            assert!(
                rect.x > 0.01,
                "caret_rect(line.byte_end, Upstream).x must not collapse to 0: line={}, range={}..{}",
                line.id,
                line.byte_start,
                line.byte_end
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
                roundtrip, line.byte_end,
                "xToCursor(cursorToX(line.byte_end)) must return line.byte_end for line {}",
                line.id
            );
        }
    }

    #[test]
    fn test_qchar_offset_to_byte_offset() {
        // Normal ASCII text
        let text = "hello";
        assert_eq!(qchar_offset_to_byte_offset(text, 0), 0);
        assert_eq!(qchar_offset_to_byte_offset(text, 1), 1);
        assert_eq!(qchar_offset_to_byte_offset(text, 4), 4);
        assert_eq!(qchar_offset_to_byte_offset(text, 5), 5);

        // Emoji (surrogate pair)
        let emoji = "a😀b"; // a: 1 qchar (1 byte), 😀: 2 qchars (4 bytes), b: 1 qchar (1 byte)
        assert_eq!(qchar_offset_to_byte_offset(emoji, 0), 0); // 'a'
        assert_eq!(qchar_offset_to_byte_offset(emoji, 1), 1); // '😀'
        assert_eq!(qchar_offset_to_byte_offset(emoji, 2), 5); // Fallback inside surrogate -> next char 'b'
        assert_eq!(qchar_offset_to_byte_offset(emoji, 3), 5); // 'b'
        assert_eq!(qchar_offset_to_byte_offset(emoji, 4), 6); // End

        // Out-of-bounds inputs
        assert_eq!(qchar_offset_to_byte_offset(text, 100), 5); // Fallback to text.len()
        assert_eq!(qchar_offset_to_byte_offset(emoji, 10), 6); // Fallback to emoji.len()

        // Empty string
        assert_eq!(qchar_offset_to_byte_offset("", 0), 0);
        assert_eq!(qchar_offset_to_byte_offset("", 1), 0);
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
        let line = snapshot.lines.first().expect("lines should not be empty");
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
        let boundary = snapshot.lines[0].byte_end;
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
                1,
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
                1,
            )
            .clone();
        assert!(
            snapshot.lines.len() >= 3,
            "text must wrap into >= 3 lines, got {}",
            snapshot.lines.len()
        );
        assert_line_end_roundtrip(&snapshot);
        for (idx, line) in snapshot.lines.iter().enumerate() {
            if line.byte_start == line.byte_end || line.para_text.is_empty() {
                continue;
            }
            let x_end = qtextlayout_cursor_to_x_on_line(
                &line.para_text,
                line.byte_end,
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
                line.byte_start,
                line.byte_end
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
                roundtrip, line.byte_end,
                "line {}: xToCursor(cursorToX(line.byte_end={})) returned {}",
                idx, line.byte_end, roundtrip
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
        let snapshot = layout.snapshot(text, params_large(820.0), 1).clone();
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
        let snapshot = layout.snapshot(text, params_large(820.0), 1).clone();
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
        let snapshot = layout.snapshot(text, params_large(600.0), 1).clone();
        assert!(
            snapshot.lines.len() >= 2,
            "must wrap at width=600 fontSize=45"
        );
        for line in &snapshot.lines {
            if line.byte_start == line.byte_end || line.para_text.is_empty() {
                continue;
            }
            let x_end = qtextlayout_cursor_to_x_on_line(
                &line.para_text,
                line.byte_end,
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
                line.byte_start,
                line.byte_end,
                x_end
            );
            let rect = caret_rect(
                &snapshot,
                line.byte_end,
                CaretAffinity::Upstream,
                0.0,
                800.0,
            );
            assert!(
                rect.x > 1.0,
                "caret_rect(line.byte_end, Upstream).x must be > 1.0: line_id={}, rect.x={:.4}",
                line.id,
                rect.x
            );
            assert_eq!(
                rect.visual_line_id, line.id,
                "caret_rect(line.byte_end, Upstream) must stay on source line: line_id={}, got visual_line_id={}",
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
                1,
            )
            .clone();
        assert!(snapshot.lines.len() >= 3);
        for line in &snapshot.lines {
            if line.byte_start == line.byte_end || line.para_text.is_empty() {
                continue;
            }
            let mid_byte = line.byte_start + (line.byte_end - line.byte_start) / 2;
            let mid_cursor = text.floor_char_boundary(mid_byte);
            let mid_cursor = mid_cursor.max(line.byte_start).min(line.byte_end);
            if mid_cursor == line.byte_start || mid_cursor == line.byte_end {
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
                line.byte_end,
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
                1,
            )
            .clone();
        assert!(snapshot.lines.len() >= 2);
        for line in &snapshot.lines {
            if line.byte_start == line.byte_end || line.para_text.is_empty() {
                continue;
            }
            let recomputed = qtextlayout_cursor_to_x_on_line(
                &line.para_text,
                line.byte_end,
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

    // ── Mixed-script regression tests ──
    // These cover the cases specified in the acceptance criteria:
    //   ]"  ]"  中]"  中英文abc]"混排  emoji  全角标点

    #[test]
    fn mixed_script_close_quote_roundtrip() {
        init_qt();
        // "]\"" — closing Chinese quote after ASCII bracket
        let text = "]\"";
        let snapshot = snapshot_for(text, 320.0);
        assert_line_end_roundtrip(&snapshot);
        // Verify cursor at every position is valid
        for byte_pos in 0..=text.len() {
            let pos = text.floor_char_boundary(byte_pos);
            let rect = caret_rect(&snapshot, pos, CaretAffinity::Downstream, 0.0, 800.0);
            assert!(
                rect.x > 0.0 || pos == 0,
                "cursor at byte {} must have x > 0, got {:.4}",
                pos,
                rect.x
            );
        }
    }

    #[test]
    fn mixed_script_open_quote_roundtrip() {
        init_qt();
        // "]\"" — opening Chinese quote after ASCII bracket
        let text = "]\"";
        let snapshot = snapshot_for(text, 320.0);
        assert_line_end_roundtrip(&snapshot);
    }

    #[test]
    fn mixed_script_chinese_close_quote_roundtrip() {
        init_qt();
        // "中]\"" — Chinese char + bracket + closing quote
        let text = "中]\"";
        let snapshot = snapshot_for(text, 320.0);
        assert_line_end_roundtrip(&snapshot);
        for byte_pos in 0..=text.len() {
            let pos = text.floor_char_boundary(byte_pos);
            let rect = caret_rect(&snapshot, pos, CaretAffinity::Downstream, 0.0, 800.0);
            assert!(
                rect.x >= 0.0,
                "cursor at byte {} must have x >= 0, got {:.4}",
                pos,
                rect.x
            );
        }
    }

    #[test]
    fn mixed_script_full_mixed_roundtrip() {
        init_qt();
        // "中英文abc]\"混排" — full mixed-script with closing quote
        let text = "中英文abc]\"混排";
        let snapshot = snapshot_for(text, 320.0);
        assert_line_end_roundtrip(&snapshot);
        // Verify hit_test and caret_rect agree for every position
        for line in &snapshot.lines {
            if line.byte_start == line.byte_end || line.para_text.is_empty() {
                continue;
            }
            let mid_byte = line.byte_start + (line.byte_end - line.byte_start) / 2;
            let mid_cursor = text
                .floor_char_boundary(mid_byte)
                .max(line.byte_start)
                .min(line.byte_end);
            let rect = caret_rect(&snapshot, mid_cursor, CaretAffinity::Downstream, 0.0, 800.0);
            let (hit, _affinity) = hit_test(&snapshot, rect.x + 1.0, line.y + 2.0, 0.0);
            let hit_rect = caret_rect(&snapshot, hit, CaretAffinity::Downstream, 0.0, 800.0);
            assert_eq!(
                hit_rect.visual_line_id, line.id,
                "hit_test and caret_rect must agree on visual_line_id for mixed-script text"
            );
        }
    }

    #[test]
    fn emoji_layout_roundtrip() {
        init_qt();
        let text = "你好🙂世界🎉测试";
        let snapshot = snapshot_for(text, 320.0);
        assert_line_end_roundtrip(&snapshot);
        // Verify cursor at every char boundary
        for (byte_pos, _ch) in text.char_indices() {
            let rect = caret_rect(&snapshot, byte_pos, CaretAffinity::Downstream, 0.0, 800.0);
            assert!(
                rect.x >= 0.0,
                "cursor at emoji text byte {} must have x >= 0, got {:.4}",
                byte_pos,
                rect.x
            );
        }
    }

    #[test]
    fn fullwidth_punctuation_roundtrip() {
        init_qt();
        // Full-width punctuation: 。，！？：；""''【】
        let text = "你好。世界！测试？混合：标点；";
        let snapshot = snapshot_for(text, 320.0);
        assert_line_end_roundtrip(&snapshot);
    }

    #[test]
    fn large_font_scroll_mixed_script() {
        init_qt();
        // Max font size scrolling with mixed script
        let text = "中英文abc]\"混排测试，验证大字号下滚动和光标定位。This is a longer paragraph to force wrapping at large font sizes. 继续中文测试。";
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
                1,
            )
            .clone();
        assert!(snapshot.lines.len() >= 2, "must wrap at fontSize=45");
        assert_line_end_roundtrip(&snapshot);
    }

    #[test]
    fn font_size_change_scroll_to_bottom() {
        init_qt();
        // Simulate font size change: layout at small font, then at large font
        let text = "第一行测试文字。第二行继续。第三行更多内容。第四行验证。第五行结束。";
        let mut layout = EditorLayout::default();

        // Small font
        let snapshot_small = layout
            .snapshot(
                text,
                LayoutParams {
                    width: 820.0,
                    font_size: 16.0,
                    font_family: "serif".to_string(),
                    line_spacing: 1.5,
                    text_indent: 32.0,
                    padding: 16.0,
                },
                1,
            )
            .clone();

        // Large font
        let snapshot_large = layout
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
                2,
            )
            .clone();

        // Both must have valid line-end roundtrips
        assert_line_end_roundtrip(&snapshot_small);
        assert_line_end_roundtrip(&snapshot_large);

        // Large font must have more lines (or equal) than small font
        assert!(
            snapshot_large.lines.len() >= snapshot_small.lines.len(),
            "large font should produce >= lines than small font"
        );

        // Cursor at end of text must be valid in both
        let rect_small = caret_rect(
            &snapshot_small,
            text.len(),
            CaretAffinity::Upstream,
            0.0,
            800.0,
        );
        let rect_large = caret_rect(
            &snapshot_large,
            text.len(),
            CaretAffinity::Upstream,
            0.0,
            800.0,
        );
        assert!(rect_small.x > 0.0, "small font end cursor x must be > 0");
        assert!(rect_large.x > 0.0, "large font end cursor x must be > 0");
    }
}
