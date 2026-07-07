using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.Text;
using Microsoft.Graphics.Canvas.UI.Xaml;
using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using System;
using System.Collections.Generic;
using Windows.Foundation;
using Windows.System;
using Windows.UI;

namespace Sujian.Windows.Editor;

public sealed class SujianEditor : UserControl
{
    private readonly List<string> _lines = new() { string.Empty };
    private int _cursorLine;
    private int _cursorColumn;
    private double _scrollY;
    private float _lineHeight = 24f;
    private float _fontSize = 16f;
    private float _firstLineIndentEm = 2f;
    private CanvasTextFormat? _textFormat;
    private CanvasControl? _canvas;
    private bool _isComposing;
    private string _compositionText = string.Empty;
    private bool _cursorVisible = true;
    private DispatcherTimer? _cursorBlinkTimer;
    private int _undoIndex;
    private readonly List<string> _undoStack = new();

    public static readonly DependencyProperty TextProperty = DependencyProperty.Register(
        nameof(Text),
        typeof(string),
        typeof(SujianEditor),
        new PropertyMetadata(string.Empty, OnTextChanged));

    public static readonly DependencyProperty FontSizeSettingProperty = DependencyProperty.Register(
        nameof(FontSizeSetting),
        typeof(float),
        typeof(SujianEditor),
        new PropertyMetadata(16f, OnFontSizeSettingChanged));

    public static readonly DependencyProperty FirstLineIndentEmProperty = DependencyProperty.Register(
        nameof(FirstLineIndentEm),
        typeof(float),
        typeof(SujianEditor),
        new PropertyMetadata(2f, OnIndentChanged));

    public string Text
    {
        get => (string)GetValue(TextProperty);
        set => SetValue(TextProperty, value ?? string.Empty);
    }

    public float FontSizeSetting
    {
        get => (float)GetValue(FontSizeSettingProperty);
        set => SetValue(FontSizeSettingProperty, value);
    }

    public float FirstLineIndentEm
    {
        get => (float)GetValue(FirstLineIndentEmProperty);
        set => SetValue(FirstLineIndentEmProperty, value);
    }

    public SujianEditor()
    {
        IsTabStop = true;
        UseSystemFocusVisuals = true;
        Background = new SolidColorBrush(Colors.Transparent);
        PointerPressed += OnPointerPressed;
        KeyDown += OnKeyDown;
        CharacterReceived += OnCharacterReceived;
        PointerWheelChanged += OnPointerWheelChanged;
        GotFocus += OnGotFocus;
        LostFocus += OnLostFocus;

        _canvas = new CanvasControl();
        _canvas.CreateResources += OnCreateResources;
        _canvas.Draw += OnDraw;
        Content = _canvas;

        _cursorBlinkTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(530) };
        _cursorBlinkTimer.Tick += (_, _) => { _cursorVisible = !_cursorVisible; _canvas?.Invalidate(); };
    }

    private void OnGotFocus(object sender, RoutedEventArgs e)
    {
        _cursorVisible = true;
        _cursorBlinkTimer?.Start();
        _canvas?.Invalidate();
    }

    private void OnLostFocus(object sender, RoutedEventArgs e)
    {
        _cursorBlinkTimer?.Stop();
        _cursorVisible = false;
        _canvas?.Invalidate();
    }

    private void OnCreateResources(CanvasControl sender, object args)
    {
        EnsureTextFormat();
    }

    private void EnsureTextFormat()
    {
        _textFormat = new CanvasTextFormat
        {
            FontSize = _fontSize,
            WordWrapping = CanvasWordWrapping.Wrap,
            LineSpacing = 1.5f,
            LineSpacingBaseline = _fontSize * 1.2f,
        };
    }

    private static void OnFontSizeSettingChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
    {
        var editor = (SujianEditor)d;
        editor._fontSize = (float)e.NewValue;
        editor.EnsureTextFormat();
        editor._canvas?.Invalidate();
    }

    private static void OnIndentChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
    {
        var editor = (SujianEditor)d;
        editor._firstLineIndentEm = (float)e.NewValue;
        editor._canvas?.Invalidate();
    }

    private static void OnTextChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
    {
        var editor = (SujianEditor)d;
        editor.LoadPlainText((string?)e.NewValue ?? string.Empty);
        editor._canvas?.Invalidate();
    }

    private void LoadPlainText(string text)
    {
        _lines.Clear();
        _lines.AddRange(text.Replace("\r\n", "\n").Replace('\r', '\n').Split('\n'));
        if (_lines.Count == 0) _lines.Add(string.Empty);
        _cursorLine = Math.Clamp(_cursorLine, 0, _lines.Count - 1);
        _cursorColumn = Math.Clamp(_cursorColumn, 0, _lines[_cursorLine].Length);
    }

    private void PushUndo()
    {
        var current = string.Join('\n', _lines);
        if (_undoStack.Count > 0 && _undoStack[_undoStack.Count - 1] == current) return;
        if (_undoIndex < _undoStack.Count)
            _undoStack.RemoveRange(_undoIndex, _undoStack.Count - _undoIndex);
        _undoStack.Add(current);
        if (_undoStack.Count > 200) _undoStack.RemoveAt(0);
        _undoIndex = _undoStack.Count;
    }

    private void CommitText(string text)
    {
        PushUndo();
        foreach (var ch in text)
        {
            if (ch == '\r') continue;
            if (ch == '\n') InsertNewLine();
            else InsertChar(ch);
        }
        PublishText();
    }

    private void InsertChar(char ch)
    {
        var line = _lines[_cursorLine];
        _lines[_cursorLine] = line.Insert(_cursorColumn, ch.ToString());
        _cursorColumn++;
    }

    private void InsertNewLine()
    {
        var line = _lines[_cursorLine];
        var left = line[.._cursorColumn];
        var right = line[_cursorColumn..];
        _lines[_cursorLine] = left;
        _lines.Insert(_cursorLine + 1, right);
        _cursorLine++;
        _cursorColumn = 0;
    }

    private void DeleteBackward()
    {
        PushUndo();
        if (_cursorColumn > 0)
        {
            var line = _lines[_cursorLine];
            _lines[_cursorLine] = line.Remove(_cursorColumn - 1, 1);
            _cursorColumn--;
        }
        else if (_cursorLine > 0)
        {
            var previousLength = _lines[_cursorLine - 1].Length;
            _lines[_cursorLine - 1] += _lines[_cursorLine];
            _lines.RemoveAt(_cursorLine);
            _cursorLine--;
            _cursorColumn = previousLength;
        }
        PublishText();
    }

    private void DeleteForward()
    {
        PushUndo();
        var line = _lines[_cursorLine];
        if (_cursorColumn < line.Length)
        {
            _lines[_cursorLine] = line.Remove(_cursorColumn, 1);
        }
        else if (_cursorLine + 1 < _lines.Count)
        {
            _lines[_cursorLine] += _lines[_cursorLine + 1];
            _lines.RemoveAt(_cursorLine + 1);
        }
        PublishText();
    }

    private void Undo()
    {
        if (_undoIndex <= 0) return;
        _undoIndex--;
        LoadPlainText(_undoStack[_undoIndex]);
        PublishText();
    }

    private void Redo()
    {
        if (_undoIndex >= _undoStack.Count) return;
        _undoIndex++;
        LoadPlainText(_undoStack[_undoIndex - 1]);
        PublishText();
    }

    private void PublishText()
    {
        var next = string.Join('\n', _lines);
        if (Text != next) SetValue(TextProperty, next);
        _canvas?.Invalidate();
    }

    private float GetFirstLineIndent()
    {
        return _firstLineIndentEm * _fontSize;
    }

    private void OnPointerPressed(object sender, PointerRoutedEventArgs e)
    {
        Focus(FocusState.Pointer);
        var point = e.GetCurrentPoint(this);
        var indent = GetFirstLineIndent();
        var y = point.Position.Y + _scrollY;
        _cursorLine = Math.Clamp((int)(y / _lineHeight), 0, _lines.Count - 1);
        var line = _lines[_cursorLine];
        if (line.Length > 0 && _textFormat != null)
        {
            var x = (float)point.Position.X - indent;
            _cursorColumn = Math.Clamp(XToCharIndex(line, Math.Max(0, x)), 0, line.Length);
        }
        else
        {
            _cursorColumn = 0;
        }
        _cursorVisible = true;
        _canvas?.Invalidate();
        e.Handled = true;
    }

    private int XToCharIndex(string line, float x)
    {
        if (_textFormat == null || line.Length == 0 || _canvas?.Device == null) return 0;
        try
        {
            using var layout = new CanvasTextLayout(
                _canvas.Device, line, _textFormat, (float)ActualWidth - GetFirstLineIndent(), _lineHeight);
            var hitTest = layout.HitTest(x, 0);
            return Math.Clamp(hitTest.CharacterIndex, 0, line.Length);
        }
        catch
        {
            return Math.Clamp((int)(x / (_fontSize * 0.6)), 0, line.Length);
        }
    }

    private void OnCharacterReceived(UIElement sender, CharacterReceivedRoutedEventArgs args)
    {
        if (args.Character is '\b' or '\r' or '\n') return;
        if (_isComposing) return;
        CommitText(args.Character.ToString());
        args.Handled = true;
    }

    private void OnKeyDown(object sender, KeyRoutedEventArgs e)
    {
        var ctrl = Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Control);
        bool isCtrl = ctrl.HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);

        if (isCtrl)
        {
            switch (e.Key)
            {
                case VirtualKey.Z:
                    Undo();
                    e.Handled = true;
                    return;
                case VirtualKey.Y:
                    Redo();
                    e.Handled = true;
                    return;
                case VirtualKey.A:
                    e.Handled = true;
                    return;
            }
        }

        switch (e.Key)
        {
            case VirtualKey.Back:
                DeleteBackward();
                e.Handled = true;
                break;
            case VirtualKey.Delete:
                DeleteForward();
                e.Handled = true;
                break;
            case VirtualKey.Enter:
                CommitText("\n");
                e.Handled = true;
                break;
            case VirtualKey.Left:
                if (_cursorColumn > 0) _cursorColumn--; else if (_cursorLine > 0) { _cursorLine--; _cursorColumn = _lines[_cursorLine].Length; }
                _canvas?.Invalidate();
                e.Handled = true;
                break;
            case VirtualKey.Right:
                if (_cursorColumn < _lines[_cursorLine].Length) _cursorColumn++; else if (_cursorLine + 1 < _lines.Count) { _cursorLine++; _cursorColumn = 0; }
                _canvas?.Invalidate();
                e.Handled = true;
                break;
            case VirtualKey.Up:
                if (_cursorLine > 0) _cursorLine--;
                _cursorColumn = Math.Clamp(_cursorColumn, 0, _lines[_cursorLine].Length);
                _canvas?.Invalidate();
                e.Handled = true;
                break;
            case VirtualKey.Down:
                if (_cursorLine + 1 < _lines.Count) _cursorLine++;
                _cursorColumn = Math.Clamp(_cursorColumn, 0, _lines[_cursorLine].Length);
                _canvas?.Invalidate();
                e.Handled = true;
                break;
            case VirtualKey.Home:
                _cursorColumn = 0;
                _canvas?.Invalidate();
                e.Handled = true;
                break;
            case VirtualKey.End:
                _cursorColumn = _lines[_cursorLine].Length;
                _canvas?.Invalidate();
                e.Handled = true;
                break;
            case VirtualKey.PageUp:
                _cursorLine = Math.Max(0, _cursorLine - (int)(ActualHeight / _lineHeight));
                _cursorColumn = Math.Clamp(_cursorColumn, 0, _lines[_cursorLine].Length);
                _canvas?.Invalidate();
                e.Handled = true;
                break;
            case VirtualKey.PageDown:
                _cursorLine = Math.Min(_lines.Count - 1, _cursorLine + (int)(ActualHeight / _lineHeight));
                _cursorColumn = Math.Clamp(_cursorColumn, 0, _lines[_cursorLine].Length);
                _canvas?.Invalidate();
                e.Handled = true;
                break;
        }
    }

    private void OnPointerWheelChanged(object sender, PointerRoutedEventArgs e)
    {
        var maxScroll = Math.Max(0, _lines.Count * _lineHeight - ActualHeight);
        _scrollY = Math.Clamp(_scrollY - e.GetCurrentPoint(this).Properties.MouseWheelDelta, 0, maxScroll);
        _canvas?.Invalidate();
        e.Handled = true;
    }

    private void OnDraw(CanvasControl sender, CanvasDrawEventArgs args)
    {
        var ds = args.DrawingSession;
        var width = (float)sender.ActualWidth;
        if (width <= 0 || _textFormat == null) return;

        var indent = GetFirstLineIndent();
        var contentWidth = width - indent;

        ds.Transform = System.Numerics.Matrix3x2.CreateTranslation(0, (float)(-_scrollY));

        var textColor = ((SolidColorBrush?)Foreground)?.Color ?? Colors.White;

        for (int i = 0; i < _lines.Count; i++)
        {
            var lineY = i * _lineHeight;
            if (lineY + _lineHeight < _scrollY) continue;
            if (lineY > _scrollY + ActualHeight) break;

            var lineText = _lines[i];
            var lineIndent = indent;

            if (!string.IsNullOrEmpty(lineText) && contentWidth > 0)
            {
                try
                {
                    using var layout = new CanvasTextLayout(
                        sender.Device, lineText, _textFormat, contentWidth, _lineHeight);
                    _lineHeight = layout.LineSpacing > 0 ? layout.LineSpacing : 24f;
                    ds.DrawTextLayout(layout, lineIndent, lineY, textColor);
                }
                catch
                {
                    ds.DrawText(lineText, lineIndent, lineY, textColor);
                }
            }
        }

        if (_isComposing && !string.IsNullOrEmpty(_compositionText))
        {
            try
            {
                var compY = _cursorLine * _lineHeight;
                using var compLayout = new CanvasTextLayout(
                    sender.Device, _compositionText, _textFormat, contentWidth, _lineHeight);
                ds.DrawTextLayout(compLayout, indent, compY, Colors.DodgerBlue);
            }
            catch { }
        }

        DrawCursor(ds, indent);
    }

    private void DrawCursor(CanvasDrawingSession ds, float indent)
    {
        if (!_cursorVisible) return;

        var cursorColor = Colors.DodgerBlue;
        float cursorX = indent;
        float cursorY = _cursorLine * _lineHeight;

        var line = _lines[_cursorLine];
        if (line.Length > 0 && _cursorColumn > 0 && _canvas?.Device != null && _textFormat != null)
        {
            try
            {
                var contentWidth = (float)ActualWidth - indent;
                using var layout = new CanvasTextLayout(
                    _canvas.Device, line, _textFormat, Math.Max(1, contentWidth), _lineHeight);
                var metrics = layout.GetCaretPosition(_cursorColumn, false);
                cursorX = indent + metrics.X;
            }
            catch { }
        }

        ds.FillRectangle(cursorX, cursorY, 2f, _lineHeight * 0.8f, cursorColor);
    }

    public Rect GetCursorRect()
    {
        float indent = GetFirstLineIndent();
        float cursorX = indent;
        float cursorY = _cursorLine * _lineHeight - (float)_scrollY;
        return new Rect(cursorX, cursorY, 2, _lineHeight * 0.8);
    }

    public void StartComposition()
    {
        _isComposing = true;
        _compositionText = string.Empty;
    }

    public void UpdateComposition(string text)
    {
        _compositionText = text;
        _canvas?.Invalidate();
    }

    public void CommitComposition(string text)
    {
        _isComposing = false;
        _compositionText = string.Empty;
        if (!string.IsNullOrEmpty(text))
        {
            CommitText(text);
        }
        else
        {
            _canvas?.Invalidate();
        }
    }

    public void CancelComposition()
    {
        _isComposing = false;
        _compositionText = string.Empty;
        _canvas?.Invalidate();
    }

    public int GetCursorLine() => _cursorLine;
    public int GetCursorColumn() => _cursorColumn;
    public int GetLineCount() => _lines.Count;
}
