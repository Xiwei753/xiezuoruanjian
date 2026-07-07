using System;
using System.Collections.Generic;

namespace Sujian.Windows.Editor.Animation;

public sealed class ActiveAnimation
{
    public ulong TransactionId { get; set; }
    public ulong? RangeId { get; set; }
    public EditorAnimationKind Kind { get; set; }
    public AnimationMode Mode { get; set; }
    public DateTime StartTime { get; set; }
    public ulong DurationMs { get; set; }
    public List<GhostGlyph> Ghosts { get; } = new();
    public bool IsFinished { get; set; }
}

public sealed class GhostGlyph
{
    public string Char { get; set; } = "";
    public float StartX { get; set; }
    public float StartY { get; set; }
    public float EndX { get; set; }
    public float EndY { get; set; }
    public float Width { get; set; }
    public float Height { get; set; }
    public float BaselineY { get; set; }
    public float Opacity { get; set; } = 1.0f;
}

public sealed class SujianAnimationController
{
    private readonly List<ActiveAnimation> _activeAnimations = new();
    private bool _animationEnabled = true;

    public bool AnimationEnabled
    {
        get => _animationEnabled;
        set
        {
            if (_animationEnabled == value) return;
            _animationEnabled = value;
            if (!value) ClearAll();
        }
    }

    public IReadOnlyList<ActiveAnimation> ActiveAnimations => _activeAnimations;

    public event EventHandler<AnimationFinishedEventArgs>? AnimationFinished;
    public event EventHandler? AnimationsChanged;

    public void ProcessTransaction(EditorVisualTransaction vt)
    {
        if (!_animationEnabled || vt.ParsedAnimationMode == AnimationMode.SystemSuppressed)
        {
            NotifySkipped(vt);
            return;
        }

        var animation = new ActiveAnimation
        {
            TransactionId = vt.Id,
            Kind = vt.ParsedKind,
            Mode = vt.ParsedAnimationMode,
            StartTime = DateTime.Now,
            DurationMs = vt.DurationMs
        };

        if (vt.HiddenVisualRanges.Count > 0)
            animation.RangeId = vt.HiddenVisualRanges[0].Id;

        switch (vt.ParsedKind)
        {
            case EditorAnimationKind.Insert:
                CreateInsertAnimation(animation, vt);
                break;
            case EditorAnimationKind.Delete:
                CreateDeleteAnimation(animation, vt);
                break;
        }

        if (animation.Ghosts.Count > 0)
        {
            _activeAnimations.Add(animation);
            AnimationsChanged?.Invoke(this, EventArgs.Empty);
        }
        else
        {
            NotifySkipped(vt);
        }
    }

    private void CreateInsertAnimation(ActiveAnimation animation, EditorVisualTransaction vt)
    {
        if (vt.InsertGlyphRects == null || vt.InsertGlyphRects.Count == 0) return;

        float startX = (float)(vt.OldCursorRect?.X ?? 0);
        float startY = (float)(vt.OldCursorRect?.BaselineY ?? 0);

        foreach (var glyph in vt.InsertGlyphRects)
        {
            animation.Ghosts.Add(new GhostGlyph
            {
                Char = glyph.Char,
                StartX = startX,
                StartY = startY,
                EndX = (float)glyph.X,
                EndY = (float)glyph.BaselineY,
                Width = (float)glyph.W,
                Height = (float)glyph.H,
                BaselineY = (float)glyph.BaselineY
            });
        }
    }

    private void CreateDeleteAnimation(ActiveAnimation animation, EditorVisualTransaction vt)
    {
        if (vt.DeletedGlyphRects == null || vt.DeletedGlyphRects.Count == 0) return;

        float endX = (float)(vt.NewCursorRect?.X ?? 0);
        float endY = (float)(vt.NewCursorRect?.BaselineY ?? 0);

        foreach (var glyph in vt.DeletedGlyphRects)
        {
            animation.Ghosts.Add(new GhostGlyph
            {
                Char = glyph.Char,
                StartX = (float)glyph.X,
                StartY = (float)glyph.BaselineY,
                EndX = endX,
                EndY = endY,
                Width = (float)glyph.W,
                Height = (float)glyph.H,
                BaselineY = (float)glyph.BaselineY
            });
        }
    }

    public void Tick()
    {
        var now = DateTime.Now;
        var toRemove = new List<ActiveAnimation>();

        foreach (var anim in _activeAnimations)
        {
            var elapsed = (now - anim.StartTime).TotalMilliseconds;
            var progress = Math.Min(1.0, elapsed / anim.DurationMs);

            foreach (var ghost in anim.Ghosts)
            {
                var eased = EaseOutCubic((float)progress);
                ghost.StartX = ghost.StartX + (ghost.EndX - ghost.StartX) * eased;
                ghost.StartY = ghost.StartY + (ghost.EndY - ghost.StartY) * eased;
                ghost.Opacity = 1.0f - eased;
            }

            if (progress >= 1.0)
            {
                anim.IsFinished = true;
                toRemove.Add(anim);
            }
        }

        foreach (var anim in toRemove)
        {
            _activeAnimations.Remove(anim);
            AnimationFinished?.Invoke(this, new AnimationFinishedEventArgs
            {
                TransactionId = anim.TransactionId,
                RangeId = anim.RangeId
            });
        }

        if (toRemove.Count > 0)
            AnimationsChanged?.Invoke(this, EventArgs.Empty);
    }

    public void ClearAll()
    {
        foreach (var anim in _activeAnimations)
        {
            AnimationFinished?.Invoke(this, new AnimationFinishedEventArgs
            {
                TransactionId = anim.TransactionId,
                RangeId = anim.RangeId
            });
        }
        _activeAnimations.Clear();
        AnimationsChanged?.Invoke(this, EventArgs.Empty);
    }

    private void NotifySkipped(EditorVisualTransaction vt)
    {
        foreach (var range in vt.HiddenVisualRanges)
        {
            AnimationFinished?.Invoke(this, new AnimationFinishedEventArgs
            {
                TransactionId = vt.Id,
                RangeId = range.Id
            });
        }
    }

    private static float EaseOutCubic(float t) => 1.0f - (1.0f - t) * (1.0f - t) * (1.0f - t);
}

public sealed class AnimationFinishedEventArgs : EventArgs
{
    public ulong TransactionId { get; set; }
    public ulong? RangeId { get; set; }
}
