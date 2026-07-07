using System;
using System.Collections.Generic;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace Sujian.Windows.Editor.Animation;

public enum AnimationMode
{
    GlyphAnimation,
    ClusterAnimation,
    RunAnimation,
    LineReflowAnimation,
    SnapshotAnimation,
    SystemSuppressed
}

public enum EditorAnimationKind
{
    Insert,
    Delete,
    Cursor
}

public sealed class GlyphRect
{
    [JsonPropertyName("x")] public double X { get; set; }
    [JsonPropertyName("y")] public double Y { get; set; }
    [JsonPropertyName("w")] public double W { get; set; }
    [JsonPropertyName("h")] public double H { get; set; }
    [JsonPropertyName("char")] public string Char { get; set; } = "";
    [JsonPropertyName("baselineY")] public double BaselineY { get; set; }
    [JsonPropertyName("byteStart")] public int ByteStart { get; set; }
    [JsonPropertyName("byteEnd")] public int ByteEnd { get; set; }
}

public sealed class ReflowGlyphRect
{
    [JsonPropertyName("char")] public string Char { get; set; } = "";
    [JsonPropertyName("byteStart")] public int ByteStart { get; set; }
    [JsonPropertyName("byteEnd")] public int ByteEnd { get; set; }
    [JsonPropertyName("oldX")] public double OldX { get; set; }
    [JsonPropertyName("oldY")] public double OldY { get; set; }
    [JsonPropertyName("oldBaselineY")] public double OldBaselineY { get; set; }
    [JsonPropertyName("newX")] public double NewX { get; set; }
    [JsonPropertyName("newY")] public double NewY { get; set; }
    [JsonPropertyName("newBaselineY")] public double NewBaselineY { get; set; }
    [JsonPropertyName("w")] public double W { get; set; }
    [JsonPropertyName("h")] public double H { get; set; }
    [JsonPropertyName("lineIndex")] public int LineIndex { get; set; }
}

public sealed class CursorRect
{
    [JsonPropertyName("x")] public double X { get; set; }
    [JsonPropertyName("top")] public double Top { get; set; }
    [JsonPropertyName("bottom")] public double Bottom { get; set; }
    [JsonPropertyName("baselineY")] public double BaselineY { get; set; }
}

public sealed class HiddenVisualRange
{
    [JsonPropertyName("id")] public ulong Id { get; set; }
    [JsonPropertyName("kind")] public string Kind { get; set; } = "";
    [JsonPropertyName("rangeStart")] public int RangeStart { get; set; }
    [JsonPropertyName("rangeEnd")] public int RangeEnd { get; set; }
}

public sealed class EditorVisualTransaction
{
    [JsonPropertyName("id")] public ulong Id { get; set; }
    [JsonPropertyName("kind")] public string Kind { get; set; } = "";
    [JsonPropertyName("cause")] public string Cause { get; set; } = "";
    [JsonPropertyName("oldText")] public string OldText { get; set; } = "";
    [JsonPropertyName("newText")] public string NewText { get; set; } = "";
    [JsonPropertyName("insertedRange")] public int[]? InsertedRange { get; set; }
    [JsonPropertyName("deletedGlyphRects")] public List<GlyphRect>? DeletedGlyphRects { get; set; }
    [JsonPropertyName("insertGlyphRects")] public List<GlyphRect>? InsertGlyphRects { get; set; }
    [JsonPropertyName("reflowGlyphRects")] public List<ReflowGlyphRect>? ReflowGlyphRects { get; set; }
    [JsonPropertyName("animationMode")] public string AnimationMode { get; set; } = "";
    [JsonPropertyName("hiddenVisualRanges")] public List<HiddenVisualRange> HiddenVisualRanges { get; set; } = new();
    [JsonPropertyName("oldCursorRect")] public CursorRect? OldCursorRect { get; set; }
    [JsonPropertyName("newCursorRect")] public CursorRect? NewCursorRect { get; set; }
    [JsonPropertyName("durationMs")] public ulong DurationMs { get; set; }

    public EditorAnimationKind ParsedKind => Kind switch
    {
        "Insert" => EditorAnimationKind.Insert,
        "Delete" => EditorAnimationKind.Delete,
        _ => EditorAnimationKind.Cursor
    };

    public AnimationMode ParsedAnimationMode => AnimationMode switch
    {
        "GlyphAnimation" => AnimationMode.GlyphAnimation,
        "ClusterAnimation" => AnimationMode.ClusterAnimation,
        "RunAnimation" => AnimationMode.RunAnimation,
        "LineReflowAnimation" => AnimationMode.LineReflowAnimation,
        "SnapshotAnimation" => AnimationMode.SnapshotAnimation,
        _ => AnimationMode.SystemSuppressed
    };

    public static EditorVisualTransaction? FromJson(string json)
    {
        try
        {
            return JsonSerializer.Deserialize<EditorVisualTransaction>(json, new JsonSerializerOptions
            {
                PropertyNameCaseInsensitive = true
            });
        }
        catch
        {
            return null;
        }
    }
}
