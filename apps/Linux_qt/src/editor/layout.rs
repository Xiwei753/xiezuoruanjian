use cpp::cpp;
use qmetaobject::QString;

// ── Qt 文本布局模块 ──
//
// 坐标空间约定：
// - Qt 层（本模块）：QChar index（UTF-16 code unit），与 QTextLayout/QTextLine API 一致
// - Core 层：UTF-8 byte offset
// - 转换入口：`sujian_editor_item` 中的 `utf8_to_utf16` / `utf16_to_utf8`
//   在调用本模块函数前完成坐标转换
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

    // ── Issue #658: 段落布局缓存 ──
    // layout 阶段（editor_prepare_paragraph_visual_snapshot）创建 QTextLayout 后
    // 存入缓存，rebuild 阶段（rebuild_text_node_from_paragraphs）读取已排好的
    // layout，不在 Scene Graph 阶段重新 beginLayout/createLine/endLayout。
    // 该变量在 editor_prepare_paragraph_visual_snapshot 中使用后由
    // rebuild_text_node_from_paragraphs 读取，两者在 GUI 线程上严格串行。
    //
    // Issue #658 评论 5620035970 问题 2: 用 generation 隔离静态正文和动画/IME 布局，
    // 两条路径各自分配独立 generation，写到独立 generation 的 cache，互不清空。
    // 渲染时用 (generation, slot) 查找 layout。
    struct LayoutGeneration {
        uint64_t generation;
        std::vector<QTextLayout*> layouts;
    };
    static std::vector<LayoutGeneration> g_layout_generations;
    static uint64_t g_next_generation = 1;
    // Issue #658 评论 5621512329 问题 1: 不再用固定数量阈值强制淘汰最老 generation。
    // generation 生命周期由持有方显式管理：
    // - 静态正文：EditorLayout 持有 current_generation，invalidate/snapshot 失效时 clear。
    // - 动画/IME 临时 generation：build_editor_layout_snapshot /
    //   build_virtual_layout_snapshot / record_visual_transaction 在提取完
    //   canonical line/image/cursor 数据后显式 clear_layout_generation 释放。
    // 这样避免仍被静态正文引用的 QTextLayout 被提前删掉。

    static std::vector<QTextLayout*>* find_layout_generation(uint64_t gen) {
        for (auto& lg : g_layout_generations) {
            if (lg.generation == gen) {
                return &lg.layouts;
            }
        }
        return nullptr;
    }

    static std::vector<QTextLayout*>* ensure_layout_generation(uint64_t gen) {
        auto* existing = find_layout_generation(gen);
        if (existing) return existing;
        g_layout_generations.push_back(LayoutGeneration{gen, {}});
        return &g_layout_generations.back().layouts;
    }

    uint64_t begin_layout_generation() {
        return g_next_generation++;
    }

    void clear_layout_generation(uint64_t gen) {
        for (auto it = g_layout_generations.begin(); it != g_layout_generations.end(); ++it) {
            if (it->generation == gen) {
                for (auto* l : it->layouts) {
                    delete l;
                }
                g_layout_generations.erase(it);
                return;
            }
        }
    }

    void clear_all_layout_generations() {
        for (auto& lg : g_layout_generations) {
            for (auto* l : lg.layouts) {
                delete l;
            }
        }
        g_layout_generations.clear();
    }

    QTextLayout* get_paragraph_layout(uint64_t gen, int slot) {
        auto* layouts = find_layout_generation(gen);
        if (!layouts) return nullptr;
        if (slot < 0 || slot >= (int)layouts->size()) return nullptr;
        return (*layouts)[slot];
    }

    // Issue #658 评论 5621512329 问题 2: 光标/命中不再重新 new QTextLayout 重新排版，
    // 直接从已排好的 generation cache 中取 QTextLine 调用 cursorToX / xToCursor。
    // gen/slot 对应的 layout 由 EditorLayout 生命周期管理（invalidate 时 clear），
    // 调用方保证 snapshot.layout_generation 在调用时有效。
    double get_paragraph_layout_cursor_to_x_on_line(
        uint64_t gen, int slot, int qline, int cursor_qchar, bool use_trailing
    ) {
        QTextLayout* layout = get_paragraph_layout(gen, slot);
        if (!layout) return 0.0;
        if (qline < 0 || qline >= layout->lineCount()) return 0.0;
        QTextLine line = layout->lineAt(qline);
        if (!line.isValid()) return 0.0;
        int line_start = line.textStart();
        int line_end = line_start + line.textLength();
        int pos = cursor_qchar;
        if (pos < line_start) pos = line_start;
        if (pos > line_end) pos = line_end;
        return line.cursorToX(pos, use_trailing ? QTextLine::Trailing : QTextLine::Leading);
    }

    int get_paragraph_layout_x_to_cursor_on_line(
        uint64_t gen, int slot, int qline, double x
    ) {
        QTextLayout* layout = get_paragraph_layout(gen, slot);
        if (!layout) return 0;
        if (qline < 0 || qline >= layout->lineCount()) return 0;
        QTextLine line = layout->lineAt(qline);
        if (!line.isValid()) return 0;
        int line_start = line.textStart();
        int line_end = line_start + line.textLength();
        int pos = line.xToCursor(x);
        if (pos < line_start) pos = line_start;
        if (pos > line_end) pos = line_end;
        return pos;
    }

    // Issue #658 评论 5620035970 问题 1+2: 按 generation + cache_slot 写指定位置，
    // 不再 push_back。空段落也占一个明确的 null slot，
    // 保持 cache_slot 与文档段落一一对应，避免越界访问。
    void set_paragraph_layout_slot_gen(uint64_t gen, int cache_slot, QTextLayout* layout) {
        auto* layouts = ensure_layout_generation(gen);
        while ((int)layouts->size() <= cache_slot) {
            layouts->push_back(nullptr);
        }
        if ((*layouts)[cache_slot]) {
            delete (*layouts)[cache_slot];
        }
        (*layouts)[cache_slot] = layout;
    }

    void set_null_paragraph_layout_slot_gen(uint64_t gen, int cache_slot) {
        auto* layouts = ensure_layout_generation(gen);
        while ((int)layouts->size() <= cache_slot) {
            layouts->push_back(nullptr);
        }
        if ((*layouts)[cache_slot]) {
            delete (*layouts)[cache_slot];
        }
        (*layouts)[cache_slot] = nullptr;
    }

    // 兼容旧调用：clear_paragraph_layout_cache 清全部 generation。
    void clear_paragraph_layout_cache() {
        clear_all_layout_generations();
    }

    /// 单行排版结果 — 跨 C++/Rust 边界的数据结构。
    ///
    /// 所有 QChar index 为 UTF-16 code unit offset（与 QTextLayout API 一致），
    /// 传入 Core 前必须转换为 UTF-8 byte offset。
    ///
    /// - `qcharStart/qcharEnd`：该行在段落中的 QChar 范围（半开区间）
    /// - `width`：行宽（物理像素）
    /// - `xPos`：行在段落中的水平起始位置（首行缩进时 > 0）
    /// - `xEndLeading/xEndTrailing`：行尾位置（含/不含 trailing whitespace）
    /// - `naturalTextWidth`：自然文本宽度（不含对齐拉伸）
    /// - `ascent/descent`：字体度量（物理像素，用于光标高度计算）
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
    /// - `xPos`：glyph 左边缘 x 坐标（物理像素，文档坐标系）
    /// - `width`：glyph 前进宽度（物理像素）
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
        entry.xPos = (qtextline_idx == 0) ? 0.0 : 0.0; // 缩进由调用方处理
        entry.width = line.naturalTextWidth();
        entry.height = line.height();
        entry.ascent = line.ascent();
        entry.descent = line.descent();
        entry.y = line.y();
        entry.xEndLeading = line.cursorToX(entry.qcharEnd, QTextLine::Leading);
        entry.xEndTrailing = line.cursorToX(entry.qcharEnd, QTextLine::Trailing);

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

        // 2. 提取 glyphRuns 和 clusters
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

        entry.cursorXMapStart = (int)g_cursor_x_map_buf.size();
        entry.cursorXMapCount = 0;
        for (int qpos = entry.qcharStart; qpos <= entry.qcharEnd; qpos++) {
            CursorXMapEntry me;
            me.qcharPos = qpos;
            me.xLeading = line.cursorToX(qpos, QTextLine::Leading);
            me.xTrailing = line.cursorToX(qpos, QTextLine::Trailing);
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
            entry.xEndLeading = line.cursorToX(entry.qcharEnd, QTextLine::Leading);
            entry.xEndTrailing = line.cursorToX(entry.qcharEnd, QTextLine::Trailing);

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

            if (generate_animation_visuals) {
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
            } // end if (generate_animation_visuals)

            entry.clusterStartIndex = clusterStartIdx;
            entry.clusterCount = (int)g_canonical_cluster_buf.size() - clusterStartIdx;

            entry.cursorXMapStart = (int)g_cursor_x_map_buf.size();
            entry.cursorXMapCount = 0;
            for (int qpos = entry.qcharStart; qpos <= entry.qcharEnd; qpos++) {
                CursorXMapEntry me;
                me.qcharPos = qpos;
                me.xLeading = line.cursorToX(qpos, QTextLine::Leading);
                me.xTrailing = line.cursorToX(qpos, QTextLine::Trailing);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaretAffinity {
    Upstream,
    Downstream,
}

/// 视觉行 — 排版后的单行文本，同时持有 UTF-8 byte offset 和 QChar (UTF-16) offset。
///
/// 坐标空间：x/y/width/height 为文档坐标系（不含 scroll offset）。
/// byte_start/byte_end 为半开区间 [byte_start, byte_end)（UTF-8 byte offset，文档级）。
/// qchar_start/qchar_end 为半开区间 [qchar_start, qchar_end)（QChar index，文档级）。
///
/// 段落相关字段（para_text, para_start, para_qchar_start/end, qtextline_idx,
/// line_wrap_width, line_indent_x, para_indent, x_end_trailing）用于
/// 重新调用 QTextLayout API 做精确光标定位和 hit test。
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
    /// Issue #658: 该视觉行所属段落在 g_paragraph_layout_cache 中的稳定 slot 索引。
    /// 按段落在文档中出现的顺序（含空段落）从 0 递增分配，与 cache slot 一一对应。
    /// scene_graph_renderer 直接消费此字段，不再自行计数 cache_idx。
    pub cache_slot: i32,
}

/// 光标矩形 — 文档坐标系（不含 scroll offset）。
/// visible=false 表示光标在可视区域外，平台端不应绘制。
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

/// 布局快照 — 某次排版结果的完整快照，与特定 text_revision 绑定。
/// text_ptr/text_len 用于快速判断文本缓冲区是否变更（指针+长度双重校验）。
/// 缓存失效条件：revision、指针、长度、宽度、字号、字体、行距、缩进或内边距任一变化。
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
    /// Issue #658 评论 5620035970 问题 2: 布局 generation — 标识本 snapshot
    /// 对应的 g_layout_generations 中的代。渲染时用 (generation, cache_slot) 查找 layout。
    pub layout_generation: u64,
}

/// Issue #658 评论 5622829886 问题 1: 已排好的 prepared layout 提升为 EditorLayout current。
///
/// 由 record_visual_transaction 全篇排版 new text 后构造，
/// 交给 EditorLayout::promote_prepared_layout 提升为 current，
/// 后续 EditorLayout::snapshot 发现 cache 有效直接返回，不再重新排版。
/// generation 对应的 QTextLayout 已存入 g_layout_generations[generation]，
/// 由 EditorLayout 生命周期管理（invalidate/snapshot 失效时 clear）。
/// text_revision / text_ptr / text_len 在提升时从当前 buffer.text 和
/// pipeline.text_revision() 获取，确保与 EditorLayout::snapshot 的 cache
/// 有效性检查一致。
#[derive(Clone)]
pub struct PromotedLayout {
    pub generation: u64,
    pub visual_lines: Vec<VisualLine>,
    pub width: f64,
    pub font_size: f32,
    pub font_family: String,
    pub line_spacing: f32,
    pub text_indent: f32,
    pub padding: f32,
    /// Issue #658 评论 5624570557 问题 1: 旧的 generation，在 promote 完成后释放。
    /// old 动画纹理提取完成后，等 new prepared layout 真正成为 current，再释放旧 generation。
    pub old_generation: u64,
}

/// Issue #658 评论 5624570557 问题 1: 已准备布局的只读句柄。
///
/// 暴露当前有效 `layout_generation + LayoutSnapshot` 的只读句柄，
/// 用于动画 old 帧从已有 QTextLine 提取视觉资源，不再重新排版。
/// 句柄持有者必须保证 generation 在句柄使用期间有效（不被 clear_layout_generation 释放）。
pub struct PreparedLayoutHandle<'a> {
    pub generation: u64,
    pub lines: &'a [VisualLine],
}

/// 编辑器布局引擎 — 管理 QTextLayout 排版缓存。
///
/// 线程约束：QTextLayout 只能在 GUI 线程使用，EditorLayout 不可跨线程。
/// 缓存策略：snapshot() 在参数/revision 不变时复用缓存，避免重复排版。
/// Issue #658 评论 5620035970 问题 2: 持有 current_generation，
/// invalidate 时 clear 旧 generation，重新排版时分配新 generation。
#[derive(Default)]
pub struct EditorLayout {
    cache: Option<LayoutSnapshot>,
    current_generation: u64,
}

impl EditorLayout {
    pub fn invalidate(&mut self) {
        // Issue #658 评论 5620035970 问题 2: 失效时释放旧 generation 的 layout cache。
        if self.current_generation != 0 {
            clear_layout_generation(self.current_generation);
            self.current_generation = 0;
        }
        self.cache = None;
    }

    pub fn cache(&self) -> Option<&LayoutSnapshot> {
        self.cache.as_ref()
    }

    /// Issue #658 评论 5624570557 问题 1: 获取当前有效的 prepared layout 句柄（如果存在）。
    ///
    /// 用于动画 old 帧从已有 QTextLine 提取视觉资源，不再重新排版。
    /// 返回的句柄包含 generation 和 VisualLine 数组引用，
    /// 调用方可用 `prepare_animation_visuals_from_layout` 按需提取动画资源。
    pub fn current_prepared_layout(&self) -> Option<PreparedLayoutHandle<'_>> {
        self.cache.as_ref().map(|c| PreparedLayoutHandle {
            generation: c.layout_generation,
            lines: &c.lines,
        })
    }

    /// Issue #658 评论 5622829886 问题 1: 把外部已排好的 prepared layout 提升为 current。
    ///
    /// 由 record_visual_transaction 全篇排版 new text 后调用，
    /// 把 new_generation 和 visual_lines 直接设为 current，
    /// 后续 snapshot() 发现 cache 有效（text_revision/text_ptr/text_len 匹配）直接返回，
    /// 不再重新排版同一 new text。
    /// 旧 current_generation（若存在且不同于 promoted.generation）会被 clear。
    /// text_revision / text_ptr / text_len 从当前 buffer.text 和 pipeline.text_revision()
    /// 获取，确保与 snapshot() 的 cache 有效性检查一致。
    /// Issue #658 评论 5624570557 问题 1: 同时释放 old_generation（如果非 0）。
    pub fn promote_prepared_layout(
        &mut self,
        promoted: PromotedLayout,
        text: &str,
        text_revision: u64,
    ) {
        let text_ptr = text.as_ptr() as usize;
        let text_len = text.len();
        // 释放旧 generation（若存在且不同于新 generation）
        if self.current_generation != 0 && self.current_generation != promoted.generation {
            clear_layout_generation(self.current_generation);
        }
        // Issue #658 评论 5624570557 问题 1: 释放 old_generation（old 动画使用的 generation）
        if promoted.old_generation != 0 && promoted.old_generation != promoted.generation {
            clear_layout_generation(promoted.old_generation);
        }
        self.current_generation = promoted.generation;
        self.cache = Some(LayoutSnapshot {
            text_revision,
            text_ptr,
            text_len,
            width: promoted.width,
            font_size: promoted.font_size,
            font_family: promoted.font_family,
            line_spacing: promoted.line_spacing,
            text_indent: promoted.text_indent,
            padding: promoted.padding,
            lines: promoted.visual_lines,
            layout_generation: promoted.generation,
        });
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
            // Issue #658 评论 5620035970 问题 2: 重新排版前释放旧 generation，
            // 分配新 generation，避免静态正文和动画/IME 互相清空 layout cache。
            if self.current_generation != 0 {
                clear_layout_generation(self.current_generation);
            }
            self.current_generation = begin_layout_generation();
            self.cache = None;
        }

        self.cache.get_or_insert_with(|| {
            // Issue #658 评论 5622829886 问题 2: 分离基础排版与动画视觉生成。
            // 静态正文只做基础排版（QTextLayout + VisualLine + cursor map），
            // 不生成 QImage/glyphRuns/cluster（generate_animation_visuals=false）。
            // QSGTextNode 直接消费已排好的 QTextLayout（通过 generation cache），
            // 不需要先把静态正文画进 QImage。同一正文状态的每段只调用一次
            // prepare_paragraph_layout_core，QTextLayout 存入
            // g_layout_generations[self.current_generation]。
            // 真正需要 line images 的动画/IME 路径各自分配独立 generation
            // 调 prepare_document_visual_snapshot 传 generate_animation_visuals=true。
            let doc_snapshot = prepare_document_visual_snapshot(
                text,
                text_revision,
                f64::from(params.font_size),
                &params.font_family,
                f64::from(params.line_spacing),
                f64::from(params.padding),
                f64::from(params.text_indent),
                params.width,
                1.0,
                "#000000",
                self.current_generation,
                false,
            );
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
                lines: doc_snapshot.visual_lines,
                layout_generation: self.current_generation,
            }
        })
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

    pub fn text_width(&self, text: &str, font_size: f64, font_family: &str) -> f64 {
        qtextlayout_cursor_to_x(text, text, font_size, font_family)
    }

    pub fn affinity_for_index_on_line(&self, line: &VisualLine, index: usize) -> CaretAffinity {
        affinity_for_index_on_line(line, index)
    }
}

/// Issue #658 评论 5620035970 问题 2: 分配新的布局 generation。
///
/// 封装 C++ `begin_layout_generation()`，供 layout_ops.rs / pipeline.rs 调用，
/// 避免在非平台封装目录直接调 cpp!(unsafe)。
/// SAFETY: GUI thread only; begin_layout_generation 在 GUI 线程分配新 generation。
pub fn begin_layout_generation() -> u64 {
    cpp!(unsafe [] -> u64 as "uint64_t" {
        return begin_layout_generation();
    })
}

/// Issue #658 评论 5620035970 问题 2: 释放指定 generation 的布局缓存。
///
/// 封装 C++ `clear_layout_generation(gen)`，供外部调用方在 snapshot 失效时释放旧 generation。
/// SAFETY: GUI thread only; gen 是之前 begin_layout_generation 分配的。
pub fn clear_layout_generation(gen: u64) {
    cpp!(unsafe [gen as "uint64_t"] {
        clear_layout_generation(gen);
    });
}

/// Issue #658 评论 5621512329 问题 2: 从已排好的 generation cache 中取 QTextLine，
/// 调用 cursorToX，不再重新 new QTextLayout 重新排版。
///
/// `gen` / `slot` 定位段落 layout，`qline` 是段落内的视觉行索引，
/// `cursor_qchar` 是段落内的 QChar (UTF-16) offset。
/// 返回该 cursor 在行内的 x 坐标（行局部坐标，不含行 x 偏移）。
/// 若 generation/slot/layout 不存在则返回 0.0。
/// SAFETY: GUI thread only; gen/slot 对应的 layout 由 EditorLayout 生命周期管理，
/// 调用方保证 snapshot.layout_generation 在调用时有效。
pub fn get_paragraph_layout_cursor_to_x_on_line(
    gen: u64,
    slot: i32,
    qline: i32,
    cursor_qchar: i32,
    use_trailing: bool,
) -> f64 {
    cpp!(unsafe [
        gen as "uint64_t",
        slot as "int",
        qline as "int",
        cursor_qchar as "int",
        use_trailing as "bool"
    ] -> f64 as "double" {
        return get_paragraph_layout_cursor_to_x_on_line(gen, slot, qline, cursor_qchar, use_trailing);
    })
}

/// Issue #658 评论 5621512329 问题 2: 从已排好的 generation cache 中取 QTextLine，
/// 调用 xToCursor，不再重新 new QTextLayout 重新排版。
///
/// `gen` / `slot` 定位段落 layout，`qline` 是段落内的视觉行索引，
/// `x` 是行内 x 坐标（行局部坐标，不含行 x 偏移）。
/// 返回该 x 对应的 QChar (UTF-16) offset（段落内）。
/// 若 generation/slot/layout 不存在则返回 0。
/// SAFETY: GUI thread only; gen/slot 对应的 layout 由 EditorLayout 生命周期管理，
/// 调用方保证 snapshot.layout_generation 在调用时有效。
pub fn get_paragraph_layout_x_to_cursor_on_line(gen: u64, slot: i32, qline: i32, x: f64) -> i32 {
    cpp!(unsafe [
        gen as "uint64_t",
        slot as "int",
        qline as "int",
        x as "double"
    ] -> i32 as "int" {
        return get_paragraph_layout_x_to_cursor_on_line(gen, slot, qline, x);
    })
}

/// Issue #658 评论 5624570557 问题 1+2: 从已有 generation 的 QTextLine 提取动画视觉资源（含 clusters）。
/// 按 (generation, cache_slot, qtextline_idx) 读取现成 QTextLine，
/// 不重新排版，直接 line.draw() 到 QImage 并提取 glyphRuns/clusters。
/// 返回完整的 CanonicalLineSnapshot 列表。
///
/// Issue #658 评论 5625515748 问题 1: 移除统一的 `paragraph_text` / `paragraph_document_byte_start`
/// 参数，改为从每个 VisualLine 自身的 `para_text` / `para_start` 取段落级文本与文档起点。
/// qchar_start/qchar_end 来自各段落自己的 QTextLayout，是段落内 QChar offset，
/// 必须用对应段落的 para_text 做 QChar→byte 转换，再加该段落的 para_start 得到文档级 byte range。
/// SAFETY: GUI thread only; gen/slot 对应的 layout 由 EditorLayout 生命周期管理。
pub fn prepare_animation_visuals_from_layout(
    handle: &PreparedLayoutHandle<'_>,
    line_ids: &[usize],
    dpr: f64,
    text_color: &str,
) -> Vec<CanonicalLineSnapshot> {
    let color = qmetaobject::QColor::from_name(text_color);
    let mut snapshots = Vec::new();

    for &line_id in line_ids {
        if line_id >= handle.lines.len() {
            continue;
        }
        let line = &handle.lines[line_id];
        let slot = line.cache_slot;
        let qtextline_idx = line.qtextline_idx;
        let gen = handle.generation;
        // Issue #658 评论 5625515748 问题 1: 每行用自己的 para_text/para_start 做
        // QChar→byte 转换。qchar_start/qchar_end 是段落内 QChar offset，
        // 必须对应该段落的 para_text，再加 para_start 得到文档级 byte offset。
        let para_text: &str = &line.para_text;
        let para_start = line.para_start;

        // SAFETY: GUI thread only; gen/slot 对应的 layout 由 EditorLayout 生命周期管理。
        let success = cpp!(unsafe [
            gen as "uint64_t",
            slot as "int",
            qtextline_idx as "int",
            dpr as "double",
            color as "QColor"
        ] -> bool as "bool" {
            extract_animation_visuals_from_existing_line(gen, slot, qtextline_idx, dpr, color);
            return !g_canonical_line_buf.empty();
        });

        if !success {
            continue;
        }

        // 从 C++ buffers 中读取提取的数据
        let qchar_start = get_canonical_line_qchar_start(0);
        let qchar_end = get_canonical_line_qchar_end(0);
        let x_pos = get_canonical_line_x_pos(0);
        let width = get_canonical_line_width(0);
        let ascent = get_canonical_line_ascent(0);
        let descent = get_canonical_line_descent(0);
        let x_end_trailing = get_canonical_line_x_end_trailing(0);
        let image_phys_w = get_canonical_line_image_phys_w(0);
        let image_phys_h = get_canonical_line_image_phys_h(0);
        let cluster_start = get_canonical_line_cluster_start(0);
        let cluster_count = get_canonical_line_cluster_count(0);
        let cursor_x_map_start = get_canonical_line_cursor_x_map_start(0);
        let cursor_x_map_count = get_canonical_line_cursor_x_map_count(0);

        // 提取 image
        let image = if image_phys_w > 0 && image_phys_h > 0 {
            let mut img = qmetaobject::QImage::new(
                qmetaobject::QSize {
                    width: 1,
                    height: 1,
                },
                qmetaobject::ImageFormat::ARGB32_Premultiplied,
            );
            let img_ptr = &mut img as *mut qmetaobject::QImage;
            cpp!(unsafe [img_ptr as "QImage*"] {
                if (!g_canonical_line_images.empty()) {
                    *img_ptr = g_canonical_line_images[0];
                }
            });
            Some(img)
        } else {
            None
        };

        // 提取 clusters
        let mut clusters = Vec::with_capacity(cluster_count as usize);
        for ci in 0..cluster_count {
            let cidx: i32 = (cluster_start as i32) + ci as i32;
            let c_qchar_start = get_canonical_cluster_qchar_start(cidx);
            let c_qchar_end = get_canonical_cluster_qchar_end(cidx);
            let c_src_x = get_canonical_cluster_src_x(cidx);
            let c_src_y = get_canonical_cluster_src_y(cidx);
            let c_src_w = get_canonical_cluster_src_w(cidx);
            let c_src_h = get_canonical_cluster_src_h(cidx);
            let c_glyph_count = get_canonical_cluster_glyph_count(cidx);
            let c_raw_font = get_canonical_cluster_raw_font(cidx);
            let c_is_rtl = get_canonical_cluster_is_rtl(cidx);
            let c_first_glyph = get_canonical_cluster_first_glyph(cidx);

            let doc_byte_start = qchar_offset_to_byte_offset(para_text, c_qchar_start) + para_start;
            let doc_byte_end = qchar_offset_to_byte_offset(para_text, c_qchar_end) + para_start;

            let (c_byte_start, c_byte_end) = (
                qchar_offset_to_byte_offset(para_text, c_qchar_start),
                qchar_offset_to_byte_offset(para_text, c_qchar_end),
            );
            let c_cluster_text = if c_byte_start <= c_byte_end && c_byte_end <= para_text.len() {
                para_text[c_byte_start..c_byte_end].to_string()
            } else {
                String::new()
            };

            clusters.push(CanonicalClusterSnapshot {
                document_byte_start: doc_byte_start,
                document_byte_end: doc_byte_end,
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

        // 提取 cursor_x_map
        let mut cursor_x_map = Vec::with_capacity(cursor_x_map_count as usize);
        for mi in 0..cursor_x_map_count {
            let midx: i32 = (cursor_x_map_start as i32) + mi as i32;
            let m_qchar = get_cursor_x_map_qchar(midx);
            let m_x_leading = get_cursor_x_map_x_leading(midx);
            let m_x_trailing = get_cursor_x_map_x_trailing(midx);
            cursor_x_map.push(CursorXMapEntry {
                qchar_pos: m_qchar,
                x_leading: m_x_leading,
                x_trailing: m_x_trailing,
            });
        }

        snapshots.push(CanonicalLineSnapshot {
            qchar_start,
            qchar_end,
            document_byte_start: qchar_offset_to_byte_offset(para_text, qchar_start) + para_start,
            document_byte_end: qchar_offset_to_byte_offset(para_text, qchar_end) + para_start,
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

    snapshots
}

// Helper functions to read from C++ buffers
fn get_canonical_line_qchar_start(idx: i32) -> usize {
    cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return static_cast<qulonglong>(g_canonical_line_buf[idx].qcharStart);
        return 0;
    })
}

fn get_canonical_line_qchar_end(idx: i32) -> usize {
    cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return static_cast<qulonglong>(g_canonical_line_buf[idx].qcharEnd);
        return 0;
    })
}

fn get_canonical_line_x_pos(idx: i32) -> f64 {
    cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].xPos;
        return 0.0;
    })
}

fn get_canonical_line_width(idx: i32) -> f64 {
    cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].width;
        return 0.0;
    })
}

fn get_canonical_line_ascent(idx: i32) -> f64 {
    cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].ascent;
        return 0.0;
    })
}

fn get_canonical_line_descent(idx: i32) -> f64 {
    cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].descent;
        return 0.0;
    })
}

fn get_canonical_line_x_end_trailing(idx: i32) -> f64 {
    cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].xEndTrailing;
        return 0.0;
    })
}

fn get_canonical_line_image_phys_w(idx: i32) -> i32 {
    cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].imagePhysW;
        return 0;
    })
}

fn get_canonical_line_image_phys_h(idx: i32) -> i32 {
    cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].imagePhysH;
        return 0;
    })
}

fn get_canonical_line_cluster_start(idx: i32) -> i32 {
    cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].clusterStartIndex;
        return 0;
    })
}

fn get_canonical_line_cluster_count(idx: i32) -> i32 {
    cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].clusterCount;
        return 0;
    })
}

fn get_canonical_line_cursor_x_map_start(idx: i32) -> i32 {
    cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].cursorXMapStart;
        return 0;
    })
}

fn get_canonical_line_cursor_x_map_count(idx: i32) -> i32 {
    cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].cursorXMapCount;
        return 0;
    })
}

fn get_canonical_cluster_qchar_start(idx: i32) -> usize {
    cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return static_cast<qulonglong>(g_canonical_cluster_buf[idx].qcharStart);
        return 0;
    })
}

fn get_canonical_cluster_qchar_end(idx: i32) -> usize {
    cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return static_cast<qulonglong>(g_canonical_cluster_buf[idx].qcharEnd);
        return 0;
    })
}

fn get_canonical_cluster_src_x(idx: i32) -> f64 {
    cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].sourceRectX;
        return 0.0;
    })
}

fn get_canonical_cluster_src_y(idx: i32) -> f64 {
    cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].sourceRectY;
        return 0.0;
    })
}

fn get_canonical_cluster_src_w(idx: i32) -> f64 {
    cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].sourceRectW;
        return 0.0;
    })
}

fn get_canonical_cluster_src_h(idx: i32) -> f64 {
    cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].sourceRectH;
        return 0.0;
    })
}

fn get_canonical_cluster_glyph_count(idx: i32) -> i32 {
    cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].glyphCount;
        return 0;
    })
}

fn get_canonical_cluster_raw_font(idx: i32) -> QString {
    cpp!(unsafe [idx as "int"] -> QString as "QString" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return QString::fromUtf8(g_canonical_cluster_buf[idx].rawFontFingerprint);
        return QString();
    })
}

fn get_canonical_cluster_is_rtl(idx: i32) -> bool {
    cpp!(unsafe [idx as "int"] -> bool as "bool" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].isRTL;
        return false;
    })
}

fn get_canonical_cluster_first_glyph(idx: i32) -> u32 {
    cpp!(unsafe [idx as "int"] -> u32 as "quint32" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].firstGlyphIndex;
        return 0;
    })
}

fn get_cursor_x_map_qchar(idx: i32) -> usize {
    cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
        if (idx >= 0 && idx < (int)g_cursor_x_map_buf.size())
            return static_cast<qulonglong>(g_cursor_x_map_buf[idx].qcharPos);
        return 0;
    })
}

fn get_cursor_x_map_x_leading(idx: i32) -> f64 {
    cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_cursor_x_map_buf.size())
            return g_cursor_x_map_buf[idx].xLeading;
        return 0.0;
    })
}

fn get_cursor_x_map_x_trailing(idx: i32) -> f64 {
    cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_cursor_x_map_buf.size())
            return g_cursor_x_map_buf[idx].xTrailing;
        return 0.0;
    })
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
            f64::from(snapshot.font_size),
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
                height: f64::from(snapshot.font_size) * f64::from(snapshot.line_spacing),
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
                cache_slot: 0,
            };
            &fallback
        }
    };

    let cursor_x = calculate_cursor_x_for_line(line, cursor_byte, affinity, snapshot);
    let (cursor_y_doc, cursor_h) =
        cursor_rect_for_line(line, f64::from(snapshot.font_size), &snapshot.font_family);
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
        let text_baseline =
            text_baseline_y(line, f64::from(snapshot.font_size), &snapshot.font_family);
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
    // Issue #658 评论 5621512329 问题 2: 不再 new QTextLayout 重新排版，
    // 直接从 snapshot.layout_generation + line.cache_slot 取已排好的 QTextLine，
    // 调用 xToCursor。para_text 仅用于 QChar↔byte offset 转换，不用于重新排版。
    let qchar_off = get_paragraph_layout_x_to_cursor_on_line(
        snapshot.layout_generation,
        line.cache_slot,
        line.qtextline_idx,
        relative,
    );
    let para_byte = qchar_offset_to_byte_offset(&line.para_text, qchar_off as usize);
    line.para_start + para_byte
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
        // Issue #658 评论 5621512329 问题 2: 不再 new QTextLayout 重新排版，
        // 直接从 snapshot.layout_generation + line.cache_slot 取已排好的 QTextLine，
        // 调用 cursorToX。para_text 仅用于 QChar↔byte offset 转换，不用于重新排版。
        let use_trailing = affinity == CaretAffinity::Upstream && cursor == line.byte_end;
        let cursor_in_para = cursor.saturating_sub(line.para_start);
        let cursor_qchar = byte_offset_to_qchar_offset(&line.para_text, cursor_in_para) as i32;
        let x = line.x
            + get_paragraph_layout_cursor_to_x_on_line(
                snapshot.layout_generation,
                line.cache_slot,
                line.qtextline_idx,
                cursor_qchar,
                use_trailing,
            );

        // Fallback: if cursorToX returns near-zero for a non-empty line,
        // use the cached x_end_trailing as a last resort.
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
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [para as "QString", before as "QString", fs as "float", ff as "QString"] -> f64 as "double" {
        return editor_layout_cursor_to_x(para, fs, ff, before);
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
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
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
    pub cluster_text: String,
}

#[derive(Clone)]
pub struct CursorXMapEntry {
    pub qchar_pos: usize,
    pub x_leading: f64,
    pub x_trailing: f64,
}

#[derive(Clone)]
pub struct CanonicalLineSnapshot {
    pub qchar_start: usize,
    pub qchar_end: usize,
    pub document_byte_start: usize,
    pub document_byte_end: usize,
    pub x_pos: f64,
    pub width: f64,
    pub ascent: f64,
    pub descent: f64,
    pub x_end_trailing: f64,
    pub image: Option<qmetaobject::QImage>,
    pub clusters: Vec<CanonicalClusterSnapshot>,
    pub cursor_x_map: Vec<CursorXMapEntry>,
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
    cache_slot: i32,
    line_spacing: f64,
    generation: u64,
    generate_animation_visuals: bool,
) -> CanonicalParagraphSnapshot {
    let index_map = crate::editor::paragraph_index_map::ParagraphIndexMap::build(
        paragraph_text,
        paragraph_document_byte_start,
    );

    if paragraph_text.is_empty() {
        // Issue #658: 空段落也调用 C++ 占 null slot，保持 cache_slot 与文档段落一一对应。
        let para: QString = paragraph_text.to_string().into();
        let fs = font_size as f32;
        let ff: QString = font_family.to_string().into();
        let color = qmetaobject::QColor::from_name(text_color);
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
    let color = qmetaobject::QColor::from_name(text_color);
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
    pub paragraphs: Vec<CanonicalParagraphSnapshot>,
    pub visual_lines: Vec<VisualLine>,
}

impl CanonicalDocumentVisualSnapshot {
    pub fn cursor_rect(
        &self,
        cursor_byte: usize,
        affinity: CaretAffinity,
        scroll_y: f64,
        viewport_h: f64,
    ) -> CaretRect {
        let line = self
            .visual_lines
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
                    cache_slot: 0,
                };
                &fallback
            }
        };

        let cursor_x = self.cursor_x_from_canonical(line, cursor_byte, affinity);
        let (cursor_y_doc, cursor_h) =
            cursor_rect_for_line(line, self.font_size, &self.font_family);
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

    fn cursor_x_from_canonical(
        &self,
        line: &VisualLine,
        cursor_byte: usize,
        affinity: CaretAffinity,
    ) -> f64 {
        if line.para_text.is_empty() {
            if line.width > 0.0 && cursor_byte == line.byte_end {
                return line.x + line.width;
            }
            return line.x;
        }

        let cursor_in_para = cursor_byte.saturating_sub(line.para_start);
        let para = match self.paragraphs.iter().find(|p| {
            p.paragraph_document_byte_start <= cursor_byte
                && cursor_byte <= p.paragraph_document_byte_start + p.paragraph_text.len()
        }) {
            Some(p) => p,
            None => return line.x,
        };

        let cursor_qchar = byte_offset_to_qchar_offset(&para.paragraph_text, cursor_in_para);

        let canonical_line = para
            .lines
            .iter()
            .find(|cl| cl.qchar_start <= cursor_qchar && cursor_qchar <= cl.qchar_end);

        match canonical_line {
            Some(cl) => {
                let use_trailing =
                    affinity == CaretAffinity::Upstream && cursor_byte == line.byte_end;
                let entry = cl.cursor_x_map.iter().find(|m| m.qchar_pos == cursor_qchar);
                let x_in_line = match entry {
                    Some(m) => {
                        if use_trailing {
                            m.x_trailing
                        } else {
                            m.x_leading
                        }
                    }
                    None => {
                        if let Some(last) = cl.cursor_x_map.last() {
                            if use_trailing {
                                last.x_trailing
                            } else {
                                last.x_leading
                            }
                        } else {
                            0.0
                        }
                    }
                };
                let x = line.x + x_in_line;

                if x <= line.x + 0.5
                    && line.byte_start != line.byte_end
                    && affinity == CaretAffinity::Upstream
                    && cursor_byte == line.byte_end
                    && line.x_end_trailing > 0.0
                {
                    return line.x + line.x_end_trailing;
                }

                x
            }
            None => line.x,
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
            // Issue #658 评论 5620035970 问题 2: 动画路径不直接渲染此 snapshot，
            // generation 填 0 表示不用于 rebuild_text_node_from_paragraphs。
            layout_generation: 0,
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
    generation: u64,
    generate_animation_visuals: bool,
) -> CanonicalDocumentVisualSnapshot {
    // Issue #658 评论 5623746506 问题 2a: 收口到受影响范围生成动画视觉。
    // affected_byte_start >= affected_byte_end 表示全篇生成（保持原语义）。
    prepare_document_visual_snapshot_impl(
        text,
        text_revision,
        font_size,
        font_family,
        line_spacing,
        padding,
        indent,
        width,
        dpr,
        text_color,
        generation,
        generate_animation_visuals,
        0,
        0,
    )
}

/// Issue #658 评论 5623746506 问题 2a: 按受影响字节范围生成动画视觉的排版入口。
///
/// 与 `prepare_document_visual_snapshot` 相同，但只有与 `[affected_byte_start,
/// affected_byte_end)` 有交集的段落才以 `generate_animation_visuals=true` 排版
/// （生成 QImage/glyphRuns/cluster）；其他段落只做基础排版
/// （`generate_animation_visuals=false`），保留 QTextLayout/VisualLine 供静态
/// QSGTextNode 消费。基础 canonical 排版仍一次生成完整 new text 的所有段落。
///
/// Issue #658 评论 5624570557 问题 2: 分离基础排版与动画视觉生成。
/// 本函数只做基础排版（QTextLayout + VisualLine + cursor map），不生成 QImage。
/// 调用方需随后调用 `prepare_animation_visuals_from_layout` 对受影响行提取动画资源。
pub fn prepare_document_visual_snapshot_scoped(
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
    generation: u64,
    affected_byte_start: usize,
    affected_byte_end: usize,
) -> CanonicalDocumentVisualSnapshot {
    // Issue #658 评论 5624570557 问题 2: 基础排版不生成动画视觉，
    // 只得到 QTextLayout/VisualLine/cursor 几何。
    // 动画视觉由 prepare_animation_visuals_from_layout 单独提取。
    prepare_document_visual_snapshot_impl(
        text,
        text_revision,
        font_size,
        font_family,
        line_spacing,
        padding,
        indent,
        width,
        dpr,
        text_color,
        generation,
        false,
        affected_byte_start,
        affected_byte_end,
    )
}

fn prepare_document_visual_snapshot_impl(
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
    generation: u64,
    generate_animation_visuals: bool,
    affected_byte_start: usize,
    affected_byte_end: usize,
) -> CanonicalDocumentVisualSnapshot {
    // Issue #658 评论 5620035970 问题 2: 不再 clear_paragraph_layout_cache()，
    // 由调用方在批次开始前分配独立 generation，本函数用 generation 写对应代的 cache，
    // 与静态正文路径互不干扰。
    // Issue #658 评论 5623746506 问题 2a: affected_byte_start < affected_byte_end
    // 时只对受影响段落生成 QImage/glyph/cluster；>= 时全篇生成（原语义）。
    let scoped_animation = generate_animation_visuals && affected_byte_start < affected_byte_end;

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
    let mut paragraph_idx: i32 = 0;

    for paragraph in text.split_inclusive('\n') {
        let hard_break = paragraph.ends_with('\n');
        let paragraph_text = paragraph.trim_end_matches('\n');

        if paragraph_text.is_empty() {
            let empty_ascent = get_font_ascent(font_family, font_size as f32);
            let empty_descent = get_font_descent(font_family, font_size as f32);
            // Issue #658: 空段落也调用 prepare_paragraph_visual_snapshot 占 null slot，
            // 保持 cache_slot 与文档段落一一对应。
            let _empty_canonical = prepare_paragraph_visual_snapshot(
                paragraph_text,
                paragraph_start,
                font_size,
                font_family,
                available,
                indent,
                dpr,
                text_color,
                paragraph_idx,
                line_spacing,
                generation,
                generate_animation_visuals,
            );
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
                cache_slot: paragraph_idx,
            });
            line_id += 1;
            y += line_height;

            paragraphs.push(CanonicalParagraphSnapshot {
                paragraph_text: String::new(),
                paragraph_document_byte_start: paragraph_start,
                lines: Vec::new(),
                index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build(
                    "",
                    paragraph_start,
                ),
            });

            paragraph_start += paragraph.len();
            paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();
            paragraph_idx += 1;
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
            paragraph_idx,
            line_spacing,
            generation,
            // Issue #658 评论 5623746506 问题 2a: scoped_animation 时只对受影响段落
            // 生成 QImage/glyph/cluster；其他段落只做基础排版。
            if scoped_animation {
                let para_end = paragraph_start + paragraph.len();
                paragraph_start < affected_byte_end && para_end > affected_byte_start
            } else {
                generate_animation_visuals
            },
        );
        paragraph_idx += 1;

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
                line_wrap_width: if is_first {
                    available - indent
                } else {
                    available
                },
                line_indent_x: if is_first { indent } else { 0.0 },
                para_indent: indent,
                x_end_trailing: canonical_line.x_end_trailing,
                qt_ascent: canonical_line.ascent,
                qt_descent: canonical_line.descent,
                cache_slot: paragraph_idx - 1,
            });
            line_id += 1;
            y += actual_line_h;
        }

        paragraph_start += paragraph.len();
        paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();

        paragraphs.push(canonical);
    }

    // Issue #658 评论 5622188166 问题 2: 空文本处理，与 layout_lines 行为一致。
    // "".split_inclusive('\n') 返回空迭代器，需在此补充一个空段 VisualLine，
    // 保证 snapshot.lines 非空，让 hit_test/caret_rect 有行可操作。
    if text.is_empty() {
        let empty_ascent = get_font_ascent(font_family, font_size as f32);
        let empty_descent = get_font_descent(font_family, font_size as f32);
        let _empty_canonical = prepare_paragraph_visual_snapshot(
            "",
            0,
            font_size,
            font_family,
            available,
            indent,
            dpr,
            text_color,
            0,
            line_spacing,
            generation,
            generate_animation_visuals,
        );
        visual_lines.push(VisualLine {
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
            qt_ascent: empty_ascent,
            qt_descent: empty_descent,
            cache_slot: 0,
        });
        paragraphs.push(CanonicalParagraphSnapshot {
            paragraph_text: String::new(),
            paragraph_document_byte_start: 0,
            lines: Vec::new(),
            index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build("", 0),
        });
    }

    if text.ends_with('\n') {
        let text_qchar_len: usize = text.chars().map(|c| c.len_utf16()).sum();
        // Issue #658: 尾部换行产生的空段也占 null slot。
        let _empty_canonical = prepare_paragraph_visual_snapshot(
            "",
            text.len(),
            font_size,
            font_family,
            available,
            indent,
            dpr,
            text_color,
            paragraph_idx,
            line_spacing,
            generation,
            generate_animation_visuals,
        );
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
            cache_slot: paragraph_idx,
        });
        // paragraph_idx 递增省略：尾部换行分支后不再使用。
    }

    CanonicalDocumentVisualSnapshot {
        text_revision,
        font_size,
        font_family: font_family.to_string(),
        line_spacing,
        text_indent: indent,
        padding,
        width,
        dpr,
        paragraphs,
        visual_lines,
    }
}

/// Issue #658 评论 5625515748 问题 2: 只从已有 VisualLine 组装 Rust 数据构造
/// `CanonicalDocumentVisualSnapshot`，不调用任何 QTextLayout/beginLayout/createLine。
///
/// 与 `prepare_document_visual_snapshot` 的区别：本函数不重新排版，直接把 `lines` 中
/// 每个 `VisualLine` 携带的 Rust 几何数据（byte_start/byte_end/qchar/x/y/width/height
/// /ascent/descent 等）填入 `visual_lines` 和对应段落的 `CanonicalLineSnapshot`。
/// `image` / `clusters` / `cursor_x_map` 初始为空，随后由
/// `inject_animation_visuals_into_snapshot` 填充 image/clusters。
///
/// 用途：动画 old 帧从已有 prepared layout 的 VisualLine 组装 old_doc_snapshot，
/// 避免对 old text 重新排版。`cursor_rect` 依赖 `visual_lines` 的 Rust 几何数据
/// （y/height/font_size/font_family）计算 cursor_y/h 和 baseline；`cursor_x`
/// 在 `cursor_x_map` 为空时退化为 `line.x`（行首），对 old 动画起点可接受。
///
/// `padding` 用于从 `VisualLine.x`（含 padding）还原 `CanonicalLineSnapshot.x_pos`
/// （不含 padding）：`x_pos = line.x - padding`（与 `prepare_document_visual_snapshot_impl`
/// line 3320 `x = padding + canonical_line.x_pos` 对应）。
pub fn assemble_document_visual_snapshot_from_lines(
    lines: &[VisualLine],
    text_revision: u64,
    font_size: f64,
    font_family: &str,
    line_spacing: f64,
    text_indent: f64,
    padding: f64,
    width: f64,
    dpr: f64,
) -> CanonicalDocumentVisualSnapshot {
    let mut paragraphs: Vec<CanonicalParagraphSnapshot> = Vec::new();

    for line in lines {
        // 空段落（para_text 为空）不构造 CanonicalLineSnapshot：
        // build_from_canonical_document 对空段落直接跳过（line_snapshot_builder.rs:42-69），
        // 且 prepare_animation_visuals_from_layout 对空段落不会产生 anim_line。
        if line.para_text.is_empty() {
            continue;
        }

        // 按 para_start 找到或创建对应 CanonicalParagraphSnapshot。
        // paragraphs 顺序按首次出现的 para_start，与 prepare_document_visual_snapshot_impl
        // 按文档顺序遍历段落一致。
        let para_idx = if let Some(idx) = paragraphs
            .iter()
            .position(|p| p.paragraph_document_byte_start == line.para_start)
        {
            idx
        } else {
            paragraphs.push(CanonicalParagraphSnapshot {
                paragraph_text: line.para_text.clone(),
                paragraph_document_byte_start: line.para_start,
                lines: Vec::new(),
                index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build(
                    &line.para_text,
                    line.para_start,
                ),
            });
            // 刚 push 成功，新索引 = len - 1，逻辑上一定 < len。
            paragraphs.len() - 1
        };

        let para = &mut paragraphs[para_idx];
        // x_pos = line.x - padding（与 prepare_document_visual_snapshot_impl
        // `x: padding + canonical_line.x_pos` 对应）。
        let x_pos = line.x - padding;
        para.lines.push(CanonicalLineSnapshot {
            // para_qchar_start/para_qchar_end 是段落内 QChar offset，
            // 与 prepare_paragraph_visual_snapshot 产生的 canonical_line.qchar_start/end 一致。
            qchar_start: line.para_qchar_start,
            qchar_end: line.para_qchar_end,
            // document_byte_start/end 直接用 VisualLine 的文档级 byte offset，
            // 与 prepare_animation_visuals_from_layout 产生的 anim_line.document_byte_start
            // （= qchar_offset_to_byte_offset(para_text, qchar_start) + para_start）一致，
            // 保证 inject_animation_visuals_into_snapshot 能按 document_byte_start 匹配注入。
            document_byte_start: line.byte_start,
            document_byte_end: line.byte_end,
            x_pos,
            width: line.width,
            ascent: line.qt_ascent,
            descent: line.qt_descent,
            x_end_trailing: line.x_end_trailing,
            // image/clusters 初始为空，由 inject_animation_visuals_into_snapshot 填充。
            image: None,
            clusters: Vec::new(),
            // cursor_x_map 为空：VisualLine 不携带 cursor_x_map（需 C++ QTextLayout 提取）。
            // cursor_x_from_canonical 在 cursor_x_map 为空时退化为 line.x（行首），
            // 对 old 动画起点可接受（动画主要看 new cursor 和 glyph rects）。
            cursor_x_map: Vec::new(),
        });
    }

    CanonicalDocumentVisualSnapshot {
        text_revision,
        font_size,
        font_family: font_family.to_string(),
        line_spacing,
        text_indent,
        padding,
        width,
        dpr,
        paragraphs,
        // visual_lines 直接 clone，保持与原 layout 一致的 Rust 几何数据。
        visual_lines: lines.to_vec(),
    }
}

pub fn prepare_affected_paragraphs_visual_snapshot(
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
    affected_byte_start: usize,
    affected_byte_end: usize,
    previous_snapshot: Option<&CanonicalDocumentVisualSnapshot>,
    generation: u64,
    generate_animation_visuals: bool,
) -> CanonicalDocumentVisualSnapshot {
    let metrics_h = get_font_ascent(font_family, font_size as f32)
        + get_font_descent(font_family, font_size as f32);
    let line_height = (font_size * line_spacing)
        .max(font_size + 4.0)
        .max(metrics_h);
    let available = (width - padding * 2.0).max(font_size);

    let mut affected_para_indices: Vec<usize> = Vec::new();
    let paragraphs_split: Vec<&str> = text.split_inclusive('\n').collect();
    let total_paras = paragraphs_split.len();
    let mut para_start: usize = 0;
    for (para_idx, paragraph) in paragraphs_split.iter().enumerate() {
        let para_end = para_start + paragraph.len();
        if para_start < affected_byte_end && para_end > affected_byte_start {
            if !affected_para_indices.contains(&para_idx) {
                affected_para_indices.push(para_idx);
            }
            if para_idx > 0 && !affected_para_indices.contains(&(para_idx - 1)) {
                affected_para_indices.push(para_idx - 1);
            }
            if para_idx + 1 < total_paras && !affected_para_indices.contains(&(para_idx + 1)) {
                affected_para_indices.push(para_idx + 1);
            }
        }
        para_start = para_end;
    }

    if affected_para_indices.is_empty() {
        affected_para_indices.push(0);
    }

    let mut paragraphs = Vec::new();
    let mut visual_lines = Vec::new();
    let mut y: f64 = padding;
    let mut paragraph_start: usize = 0;
    let mut paragraph_qchar_start: usize = 0;
    let mut line_id: usize = 0;
    let mut current_para_idx: usize = 0;

    for paragraph in text.split_inclusive('\n') {
        let hard_break = paragraph.ends_with('\n');
        let paragraph_text = paragraph.trim_end_matches('\n');

        let is_affected = affected_para_indices.contains(&current_para_idx);

        if !is_affected {
            let mut reused_from_prev = false;
            if let Some(prev) = previous_snapshot {
                let prev_para_idx = if current_para_idx < prev.paragraphs.len()
                    && prev.paragraphs[current_para_idx].paragraph_text == paragraph_text
                {
                    Some(current_para_idx)
                } else {
                    None
                };
                let prev_lines: Vec<VisualLine> = if let Some(pidx) = prev_para_idx {
                    let prev_p = &prev.paragraphs[pidx];
                    let prev_para_byte_start = prev_p.paragraph_document_byte_start;
                    prev.visual_lines
                        .iter()
                        .filter(|l| l.para_start == prev_para_byte_start)
                        .cloned()
                        .collect()
                } else {
                    Vec::new()
                };

                if !prev_lines.is_empty() {
                    if let Some(first_prev) = prev_lines.first() {
                        let y_offset = y - first_prev.y;
                        let byte_offset = paragraph_start as i64 - first_prev.para_start as i64;
                        let qchar_offset = paragraph_qchar_start as i64
                            - first_prev.qchar_start as i64
                            + first_prev.para_qchar_start as i64;
                        for mut vl in prev_lines {
                            vl.id = line_id;
                            vl.y += y_offset;
                            if byte_offset != 0 {
                                let new_para_start = (vl.para_start as i64 + byte_offset) as usize;
                                let new_byte_start = (vl.byte_start as i64 + byte_offset) as usize;
                                let new_byte_end = (vl.byte_end as i64 + byte_offset) as usize;
                                vl.para_start = new_para_start;
                                vl.byte_start = new_byte_start;
                                vl.byte_end = new_byte_end;
                            }
                            if qchar_offset != 0 {
                                vl.qchar_start = (vl.qchar_start as i64 + qchar_offset) as usize;
                                vl.qchar_end = (vl.qchar_end as i64 + qchar_offset) as usize;
                            }
                            visual_lines.push(vl);
                            line_id += 1;
                        }
                        if let Some(last_vl) = visual_lines.last() {
                            y = last_vl.y + last_vl.height;
                        }
                        reused_from_prev = true;
                    }
                }

                if let Some(pidx) = prev_para_idx {
                    let mut para = prev.paragraphs[pidx].clone();
                    let byte_offset =
                        paragraph_start as i64 - para.paragraph_document_byte_start as i64;
                    if byte_offset != 0 {
                        para.paragraph_document_byte_start = paragraph_start;
                        para.index_map =
                            crate::editor::paragraph_index_map::ParagraphIndexMap::build(
                                &para.paragraph_text,
                                paragraph_start,
                            );
                        for line in &mut para.lines {
                            line.document_byte_start =
                                (line.document_byte_start as i64 + byte_offset) as usize;
                            line.document_byte_end =
                                (line.document_byte_end as i64 + byte_offset) as usize;
                            for cluster in &mut line.clusters {
                                cluster.document_byte_start =
                                    (cluster.document_byte_start as i64 + byte_offset) as usize;
                                cluster.document_byte_end =
                                    (cluster.document_byte_end as i64 + byte_offset) as usize;
                            }
                        }
                    }
                    paragraphs.push(para);
                } else {
                    paragraphs.push(CanonicalParagraphSnapshot {
                        paragraph_text: paragraph_text.to_string(),
                        paragraph_document_byte_start: paragraph_start,
                        lines: Vec::new(),
                        index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build(
                            paragraph_text,
                            paragraph_start,
                        ),
                    });
                }
            }

            if !reused_from_prev {
                if paragraph_text.is_empty() {
                    // Issue #658: 空段落也占 null slot。
                    let _empty_canonical = prepare_paragraph_visual_snapshot(
                        paragraph_text,
                        paragraph_start,
                        font_size,
                        font_family,
                        available,
                        indent,
                        dpr,
                        text_color,
                        current_para_idx as i32,
                        line_spacing,
                        generation,
                        generate_animation_visuals,
                    );
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
                        qt_ascent: get_font_ascent(font_family, font_size as f32),
                        qt_descent: get_font_descent(font_family, font_size as f32),
                        cache_slot: current_para_idx as i32,
                    });
                    line_id += 1;
                    y += line_height;
                } else {
                    let canonical = prepare_paragraph_visual_snapshot(
                        paragraph_text,
                        paragraph_start,
                        font_size,
                        font_family,
                        available,
                        indent,
                        dpr,
                        text_color,
                        current_para_idx as i32,
                        line_spacing,
                        generation,
                        generate_animation_visuals,
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
                            line_wrap_width: if is_first {
                                available - indent
                            } else {
                                available
                            },
                            line_indent_x: if is_first { indent } else { 0.0 },
                            para_indent: indent,
                            x_end_trailing: canonical_line.x_end_trailing,
                            qt_ascent: canonical_line.ascent,
                            qt_descent: canonical_line.descent,
                            cache_slot: current_para_idx as i32,
                        });
                        line_id += 1;
                        y += actual_line_h;
                    }
                    paragraphs.push(canonical);
                }
            }

            paragraph_start += paragraph.len();
            paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();
            current_para_idx += 1;
            continue;
        }

        if paragraph_text.is_empty() {
            let empty_ascent = get_font_ascent(font_family, font_size as f32);
            let empty_descent = get_font_descent(font_family, font_size as f32);
            // Issue #658: 空段落也占 null slot。
            let _empty_canonical = prepare_paragraph_visual_snapshot(
                paragraph_text,
                paragraph_start,
                font_size,
                font_family,
                available,
                indent,
                dpr,
                text_color,
                current_para_idx as i32,
                line_spacing,
                generation,
                generate_animation_visuals,
            );
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
                cache_slot: current_para_idx as i32,
            });
            line_id += 1;
            y += line_height;

            paragraphs.push(CanonicalParagraphSnapshot {
                paragraph_text: String::new(),
                paragraph_document_byte_start: paragraph_start,
                lines: Vec::new(),
                index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build(
                    "",
                    paragraph_start,
                ),
            });

            paragraph_start += paragraph.len();
            paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();
            current_para_idx += 1;
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
            current_para_idx as i32,
            line_spacing,
            generation,
            generate_animation_visuals,
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
                line_wrap_width: if is_first {
                    available - indent
                } else {
                    available
                },
                line_indent_x: if is_first { indent } else { 0.0 },
                para_indent: indent,
                x_end_trailing: canonical_line.x_end_trailing,
                qt_ascent: canonical_line.ascent,
                qt_descent: canonical_line.descent,
                cache_slot: current_para_idx as i32,
            });
            line_id += 1;
            y += actual_line_h;
        }

        paragraph_start += paragraph.len();
        paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();

        paragraphs.push(canonical);
        current_para_idx += 1;
    }

    if text.ends_with('\n') {
        let text_qchar_len: usize = text.chars().map(|c| c.len_utf16()).sum();
        // Issue #658: 尾部换行产生的空段也占 null slot。
        let _empty_canonical = prepare_paragraph_visual_snapshot(
            "",
            text.len(),
            font_size,
            font_family,
            available,
            indent,
            dpr,
            text_color,
            current_para_idx as i32,
            line_spacing,
            generation,
            generate_animation_visuals,
        );
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
            cache_slot: current_para_idx as i32,
        });
    }

    CanonicalDocumentVisualSnapshot {
        text_revision,
        font_size,
        font_family: font_family.to_string(),
        line_spacing,
        text_indent: indent,
        padding,
        width,
        dpr,
        paragraphs,
        visual_lines,
    }
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

/// Issue #658 评论 5624570557 问题 1: 把从已有 layout 提取的动画视觉注入到 doc snapshot。
///
/// `prepare_animation_visuals_from_layout` 从已有 QTextLine 提取的 `CanonicalLineSnapshot`
/// 包含 QImage/clusters，但 `old_doc_snapshot` 用 `generate_animation_visuals=false` 排版时
/// 其 `paragraphs[].lines[].image` 为 None。本函数按 `document_byte_start` 匹配，
/// 把提取的 QImage/clusters 注入到 doc snapshot 的对应行，使动画纹理可用。
pub fn inject_animation_visuals_into_snapshot(
    doc_snapshot: &mut CanonicalDocumentVisualSnapshot,
    animation_visuals: Vec<CanonicalLineSnapshot>,
) {
    for mut anim_line in animation_visuals {
        // 在 paragraphs 中找到包含该行的段落
        for para in &mut doc_snapshot.paragraphs {
            let para_start = para.paragraph_document_byte_start;
            let para_end = para_start + para.paragraph_text.len();
            // 检查 anim_line 是否属于该段落
            if anim_line.document_byte_start >= para_start
                && anim_line.document_byte_start < para_end
            {
                // 在该段落的 lines 中找到匹配的行（按 document_byte_start）
                for line in &mut para.lines {
                    if line.document_byte_start == anim_line.document_byte_start {
                        line.image = anim_line.image.take();
                        if !anim_line.clusters.is_empty() {
                            line.clusters = std::mem::take(&mut anim_line.clusters);
                        }
                        break;
                    }
                }
                break;
            }
        }
    }
}

/// Issue #658 评论 5626628570: old/new VisualLine 差异分类结果。
///
/// 把受影响的视觉行分成三类，避免 break 后尾部 while 把剩余全文加回 affected（Bug 1），
/// 并区分"需要重新栅格化"和"只是位置变化可复用纹理"两类行，使 y 变化的行能走
/// reflow_move 协同动画（Bug 2）。
#[derive(Clone, Debug)]
pub struct VisualLineDiff {
    /// 需要重新栅格化 QImage 的 old 行索引（内容/shaping/行内几何变化）。
    pub old_raster_line_ids: Vec<usize>,
    /// 需要重新栅格化 QImage 的 new 行索引（内容/shaping/行内几何变化）。
    pub new_raster_line_ids: Vec<usize>,
    /// 内容和 shaping 完全相同、只是 x/y 文档位置变化的 (old_idx, new_idx) 对。
    /// 不重新栅格化，复用 old 行已有 image/clusters/source rect，用 new VisualLine 的 x/y 生成终点。
    pub reusable_move_pairs: Vec<(usize, usize)>,
}

/// Issue #658 评论 5624570557 问题 2 / 评论 5626628570: 比较 old/new VisualLine，
/// 计算真正需要动画的行，并按"重新栅格化"与"复用纹理位移"分类。
///
/// 考虑换行/删换行时的受影响后续 reflow 行和相邻新旧段落。
/// 返回 [`VisualLineDiff`]，其中：
/// - `old_raster_line_ids` / `new_raster_line_ids`：内容/shaping/行内几何变化的行，需重新栅格化 QImage。
/// - `reusable_move_pairs`：内容和 shaping 完全相同、只是 x/y 文档位置变化的 (old, new) 对，
///   复用 old 行已有纹理，用 new VisualLine 的 x/y 生成 reflow_move 终点。
///
/// 算法：
/// 1. 找出字节范围相交的行（编辑点行）→ raster。
/// 2. 从编辑点行之后按 old/new byte offset 对应逐行比较：
///    - 内容/shaping 变化（width/height/qtextline_idx/字节长度）→ raster。
///    - 内容/shaping 相同、只是 x/y 位置变化 → reusable_move_pair。
///    - 完全相同（含 x/y）→ 稳定，立即停止扫描（修复 Bug 1：稳定后不再 append 剩余行）。
///    - 新增/消失的视觉行 → raster。
/// 3. 只有扫描走到某一侧末尾（未提前稳定停止）、另一侧确实还有未配对的新增/消失视觉行时，
///    才把那一侧真正未配对的尾巴加入。
/// 4. 考虑相邻段落首行缩进变化。
pub fn compare_old_new_visual_lines(
    old_lines: &[VisualLine],
    new_lines: &[VisualLine],
    inserted_range: Option<(usize, usize)>,
    deleted_range: Option<(usize, usize)>,
) -> VisualLineDiff {
    let mut old_raster_line_ids: Vec<usize> = Vec::new();
    let mut new_raster_line_ids: Vec<usize> = Vec::new();
    let mut reusable_move_pairs: Vec<(usize, usize)> = Vec::new();

    if old_lines.is_empty() || new_lines.is_empty() {
        return VisualLineDiff {
            old_raster_line_ids,
            new_raster_line_ids,
            reusable_move_pairs,
        };
    }

    // 确定受影响的字节范围
    let affected_byte_start = inserted_range
        .map(|(s, _)| s)
        .or(deleted_range.map(|(s, _)| s))
        .unwrap_or(0);
    let affected_byte_end = inserted_range
        .map(|(_, e)| e)
        .or(deleted_range.map(|(_, e)| e))
        .unwrap_or(usize::MAX);

    // 计算 old 侧受影响的行（与编辑字节范围相交）→ 必须重新栅格化
    for (idx, old_line) in old_lines.iter().enumerate() {
        let intersects =
            old_line.byte_start < affected_byte_end && old_line.byte_end > affected_byte_start;
        if intersects {
            old_raster_line_ids.push(idx);
        }
    }
    // 计算 new 侧受影响的行（与编辑字节范围相交）→ 必须重新栅格化
    for (idx, new_line) in new_lines.iter().enumerate() {
        let intersects =
            new_line.byte_start < affected_byte_end && new_line.byte_end > affected_byte_start;
        if intersects {
            new_raster_line_ids.push(idx);
        }
    }

    // 编辑点之后 old→new 的 byte offset 偏移
    let delta: isize = match (inserted_range, deleted_range) {
        (Some((ins_start, ins_end)), Some((del_start, del_end))) => {
            (ins_end - ins_start) as isize - (del_end - del_start) as isize
        }
        (Some((ins_start, ins_end)), None) => (ins_end - ins_start) as isize,
        (None, Some((del_start, del_end))) => -((del_end - del_start) as isize),
        (None, None) => 0,
    };

    // Downstream anchor: 完全在编辑区域之后的第一行索引。
    // old 侧：byte_start >= affected_byte_end（old 坐标系）
    // new 侧：byte_start >= affected_byte_end + delta（new 坐标系，delta 为字节偏移变化）
    // 替代旧的 last_direct_affected_idx + 1 方案：当 raster 集合为空时（如段尾 \n，
    // 没有任何 VisualLine 与编辑字节严格相交），旧方案 fallback 到 len()，+1 后超出
    // 边界，扫描循环 while oi < old_lines.len() 永不执行，导致 reusable_move_pairs
    // 得不到任何下游 reflow 行。
    let old_downstream_anchor = old_lines
        .iter()
        .position(|l| l.byte_start >= affected_byte_end)
        .unwrap_or(old_lines.len());
    let new_downstream_anchor = new_lines
        .iter()
        .position(|l| l.byte_start >= affected_byte_end.saturating_add_signed(delta))
        .unwrap_or(new_lines.len());

    // 双指针逐行比较：从 downstream anchor 开始，按 old/new byte offset 对应。
    // 对齐的行分三种：
    //   - 内容/shaping 变化（width/height/qtextline_idx/字节长度）→ raster（重新栅格化）
    //   - 内容/shaping 完全相同、只是 x/y 文档位置变化 → reusable_move_pair（复用纹理）
    //   - 完全相同（含 x/y）→ 稳定，立即停止扫描
    // 未对齐的行（新增/消失的视觉行）→ raster。
    // 一旦稳定停止，绝对不再 append 剩余行（修复 Bug 1）。
    // 只有扫描走到一侧末尾、且另一侧确实还有未配对的新增/消失视觉行时，才把那一侧
    // 真正未配对的尾巴加入。
    let mut oi = old_downstream_anchor;
    let mut ni = new_downstream_anchor;
    let mut stable = false;
    while oi < old_lines.len() && ni < new_lines.len() {
        let ol = &old_lines[oi];
        let nl = &new_lines[ni];
        let corr_new_byte_start = ol.byte_start.saturating_add_signed(delta);
        if nl.byte_start < corr_new_byte_start {
            // new 这行是新增的视觉行
            new_raster_line_ids.push(ni);
            ni += 1;
            continue;
        }
        if nl.byte_start > corr_new_byte_start {
            // old 这行消失了
            old_raster_line_ids.push(oi);
            oi += 1;
            continue;
        }
        // byte_start 对齐，比较视觉内容
        // same_shape: 内容/shaping 相同（不含 x/y 文档位置）
        let same_shape = (ol.width - nl.width).abs() < 0.1
            && (ol.height - nl.height).abs() < 0.1
            && ol.qtextline_idx == nl.qtextline_idx
            && (ol.byte_end - ol.byte_start) == (nl.byte_end - nl.byte_start);
        // same_pos: x/y 文档位置相同（修复 Bug 2：原 same 不比较 y）
        let same_pos = (ol.x - nl.x).abs() < 0.1 && (ol.y - nl.y).abs() < 0.1;
        if same_shape && same_pos {
            // 完全相同，稳定，停止向后扩展
            stable = true;
            break;
        }
        if same_shape {
            // 内容/shaping 完全相同，只是 x/y 文档位置变化 → 复用纹理走 reflow_move
            // 修复点 2 (Issue #658 评论 5627327573): 确保互斥——已在 raster 集合的行
            // 不进 reusable_move_pairs，避免同一行同时进入 raster 和 reusable_move。
            if !old_raster_line_ids.contains(&oi) && !new_raster_line_ids.contains(&ni) {
                reusable_move_pairs.push((oi, ni));
            }
            oi += 1;
            ni += 1;
            continue;
        }
        // 内容/shaping/行内几何变化 → 重新栅格化
        old_raster_line_ids.push(oi);
        new_raster_line_ids.push(ni);
        oi += 1;
        ni += 1;
    }
    // 只有未提前稳定停止（扫描走到一侧末尾）时，才把另一侧剩余行加入。
    // 这些是真正新增/消失的视觉行（换行/删换行导致的尾部 reflow）。
    // 修复 Bug 1：稳定停止后绝对不再 append 剩余行。
    if !stable {
        while oi < old_lines.len() {
            old_raster_line_ids.push(oi);
            oi += 1;
        }
        while ni < new_lines.len() {
            new_raster_line_ids.push(ni);
            ni += 1;
        }
    }

    // 考虑相邻段落首行缩进变化（old 侧）
    for idx in 0..old_lines.len() {
        let old_line = &old_lines[idx];
        if old_line.qtextline_idx == 0 && old_raster_line_ids.contains(&idx) {
            if let Some(new_line) = new_lines
                .iter()
                .find(|l| l.para_start == old_line.para_start && l.qtextline_idx == 0)
            {
                if (new_line.x - old_line.x).abs() > 0.1 && !old_raster_line_ids.contains(&idx) {
                    old_raster_line_ids.push(idx);
                }
            }
        }
    }
    // 考虑新段落首行（old 中不存在的段落）
    for new_line in new_lines.iter() {
        if new_line.qtextline_idx == 0 {
            let is_new_para = !old_lines
                .iter()
                .any(|l| l.para_start == new_line.para_start);
            if is_new_para {
                if let Some(old_idx) = old_lines
                    .iter()
                    .position(|l| l.para_start == new_line.para_start)
                {
                    if !old_raster_line_ids.contains(&old_idx) {
                        old_raster_line_ids.push(old_idx);
                    }
                }
            }
        }
    }

    // 考虑相邻段落首行缩进变化（new 侧）
    for idx in 0..new_lines.len() {
        let new_line = &new_lines[idx];
        if new_line.qtextline_idx == 0 && new_raster_line_ids.contains(&idx) {
            if let Some(old_line) = old_lines
                .iter()
                .find(|l| l.para_start == new_line.para_start && l.qtextline_idx == 0)
            {
                if (new_line.x - old_line.x).abs() > 0.1 && !new_raster_line_ids.contains(&idx) {
                    new_raster_line_ids.push(idx);
                }
            }
        }
    }
    // 考虑新段落首行（new 侧：new 中存在但 old 中不存在的段落）
    for (idx, new_line) in new_lines.iter().enumerate() {
        if new_line.qtextline_idx == 0 {
            let is_new_para = !old_lines
                .iter()
                .any(|l| l.para_start == new_line.para_start);
            if is_new_para && !new_raster_line_ids.contains(&idx) {
                new_raster_line_ids.push(idx);
            }
        }
    }
    // 修复点 2 (Issue #658 评论 5627327573): 保证三个集合互斥——
    // 从 reusable_move_pairs 移除任何 old_idx ∈ old_raster_line_ids
    // 或 new_idx ∈ new_raster_line_ids 的对，避免同一行同时进入 raster 和 reusable_move。
    // 同时由 sort+dedup 去重。不在返回前把互斥留给 pipeline 猜。
    reusable_move_pairs.retain(|&(o, n)| {
        !old_raster_line_ids.contains(&o) && !new_raster_line_ids.contains(&n)
    });
    old_raster_line_ids.sort_unstable();
    old_raster_line_ids.dedup();
    new_raster_line_ids.sort_unstable();
    new_raster_line_ids.dedup();
    reusable_move_pairs.sort_unstable();
    reusable_move_pairs.dedup();

    VisualLineDiff {
        old_raster_line_ids,
        new_raster_line_ids,
        reusable_move_pairs,
    }
}

