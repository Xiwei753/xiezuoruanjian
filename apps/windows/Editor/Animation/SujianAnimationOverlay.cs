using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.Text;
using Microsoft.Graphics.Canvas.UI.Xaml;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System;
using Windows.UI;

namespace Sujian.Windows.Editor.Animation;

public sealed class SujianAnimationOverlay : UserControl
{
    private CanvasControl? _canvas;
    private readonly SujianAnimationController _controller;
    private DispatcherTimer? _tickTimer;
    private Color _textColor = Colors.White;
    private float _fontSize = 16f;
    private string _fontFamily = "";

    public SujianAnimationController Controller => _controller;

    public Color TextColor
    {
        get => _textColor;
        set { _textColor = value; _canvas?.Invalidate(); }
    }

    public float FontSize
    {
        get => _fontSize;
        set { _fontSize = value; _canvas?.Invalidate(); }
    }

    public string FontFamilyName
    {
        get => _fontFamily;
        set { _fontFamily = value; _canvas?.Invalidate(); }
    }

    public SujianAnimationOverlay()
    {
        _controller = new SujianAnimationController();
        _controller.AnimationsChanged += OnAnimationsChanged;
        _controller.AnimationFinished += OnAnimationFinished;

        _canvas = new CanvasControl();
        _canvas.Draw += OnDraw;
        Content = _canvas;

        IsHitTestVisible = false;
        HorizontalAlignment = HorizontalAlignment.Stretch;
        VerticalAlignment = VerticalAlignment.Stretch;

        _tickTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(16) };
        _tickTimer.Tick += (_, _) => { _controller.Tick(); _canvas?.Invalidate(); };
    }

    public void StartTick()
    {
        _tickTimer?.Start();
    }

    public void StopTick()
    {
        _tickTimer?.Stop();
    }

    private void OnAnimationsChanged(object? sender, EventArgs e)
    {
        _canvas?.Invalidate();
    }

    private void OnAnimationFinished(object? sender, AnimationFinishedEventArgs e)
    {
        _canvas?.Invalidate();
    }

    private void OnDraw(CanvasControl sender, CanvasDrawEventArgs args)
    {
        var ds = args.DrawingSession;

        foreach (var anim in _controller.ActiveAnimations)
        {
            foreach (var ghost in anim.Ghosts)
            {
                if (ghost.CurrentOpacity <= 0.01f) continue;

                var color = Color.FromArgb(
                    (byte)(Math.Clamp(ghost.CurrentOpacity, 0f, 1f) * 255),
                    _textColor.R,
                    _textColor.G,
                    _textColor.B);

                if (!string.IsNullOrEmpty(ghost.Char) && sender.Device != null)
                {
                    try
                    {
                        using var format = new CanvasTextFormat
                        {
                            FontSize = _fontSize * ghost.CurrentScale,
                            WordWrapping = CanvasWordWrapping.NoWrap,
                        };
                        if (!string.IsNullOrEmpty(_fontFamily))
                            format.FontFamily = _fontFamily;

                        using var layout = new CanvasTextLayout(
                            sender.Device, ghost.Char, format,
                            ghost.Width * 2, ghost.Height * 2);

                        var drawX = ghost.CurrentX;
                        var drawY = ghost.CurrentY - _fontSize * ghost.CurrentScale * 0.8f;

                        ds.DrawTextLayout(layout, drawX, drawY, color);
                    }
                    catch
                    {
                        DrawFallbackRect(ds, ghost, color);
                    }
                }
                else
                {
                    DrawFallbackRect(ds, ghost, color);
                }
            }
        }
    }

    private static void DrawFallbackRect(CanvasDrawingSession ds, GhostGlyph ghost, Color color)
    {
        try
        {
            var w = ghost.Width * ghost.CurrentScale;
            var h = ghost.Height * ghost.CurrentScale;
            var x = ghost.CurrentX - (w - ghost.Width) / 2;
            var y = ghost.CurrentY - h * 0.8f;
            ds.FillRectangle(x, y, w, h, color);
        }
        catch { }
    }
}
