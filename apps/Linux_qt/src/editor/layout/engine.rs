use cpp::cpp;

// ── Qt 文本布局模块：排版引擎 ──
//
// 段落拆分、wrap、indent、line geometry、一次排版（prepare_paragraph_layout_core 等
// C++ 排版核心、prepare_paragraph_visual_snapshot）、几何 helper、坐标转换。
//
// 线程安全：`g_editor_layout_buf` 为 thread_local，
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
        // Issue #785 评论 5857873894 修改 1: clear 必须在所有 early-return 之前。
        // 之前 clear 在 get_paragraph_layout / qtextline 有效性检查之后，
        // 一旦 early-return 命中，上次残留的 g_canonical_line_buf 不会被清空，
        // Rust 侧 `!g_canonical_line_buf.empty()` 会把上次残留数据当成本次成功，
        // 导致普通可见字符输入时 InsertReveal 动画数量为 0。
        g_canonical_line_buf.clear();
        g_canonical_cluster_buf.clear();
        g_canonical_cluster_glyph_buf.clear();
        g_canonical_line_images.clear();

        QTextLayout* layout = get_paragraph_layout(gen, slot);
        if (!layout) return;
        if (qtextline_idx < 0 || qtextline_idx >= layout->lineCount()) return;

        QTextLine line = layout->lineAt(qtextline_idx);
        if (!line.isValid()) return;

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

            // Issue #810 评论 问题1: cluster 几何始终在基础排版阶段提取，
            // 不再受 generate_animation_visuals 控制——只有 QImage/纹理延迟生成。
            // 这样 canonical 排版一次就带 cluster，InsertReveal/DeleteConceal
            // 不再依赖二次提取/注入补 cluster，消除"有可见正文但 clusters 为空"
            // 的半残 canonical 状态。
            {
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
            } // end cluster extraction block (Issue #810: 始终提取 cluster)

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

// 排版几何 helper 与坐标转换移到 geometry.rs：上面这段 C++ 必须保持单块，
// 原因见 geometry.rs 的文件头注释。
mod geometry;

pub use geometry::*;
