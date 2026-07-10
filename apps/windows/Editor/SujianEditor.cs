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
using System.Linq;
using Windows.ApplicationModel.DataTransfer;
using Windows.Foundation;
using Windows.System;
using Windows.UI;
using Windows.UI.Text.Core;

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
    private bool _cursorVisible = true;
    private DispatcherTimer? _cursorBlinkTimer;
    private int _undoIndex;
    private readonly List<string> _undoStack = new();

    private int _selectionStartLine;
    private int _selectionStartColumn;
    private int _selectionEndLine;
    private int _selectionEndColumn;
    private bool _hasSelection;
    private bool _isDragging;

    private CoreTextEditContext? _editContext;
    private bool _isComposing;
    private string _compositionText = string.Empty;
    private int _compositionCursor;
    private bool _suppressNotifyTextChanged;

    private readonly IEditorTransactionBoundary _transactionBoundary;

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

    public event EventHandler? TextChangedByUser;

    public SujianEditor()
        : this(new LocalStandaloneTransactionBoundary())
    {
    }

    public SujianEditor(IEditorTransactionBoundary transactionBoundary)
    {
        _transactionBoundary = transactionBoundary ?? throw new ArgumentNullException(nameof(transactionBoundary));

        IsTabStop = true;
        UseSystemFocusVisuals = true;
        Background = new SolidColorBrush(Colors.Transparent);
        PointerPressed += OnPointerPressed;
        PointerMoved += OnPointerMoved;
        PointerReleased += OnPointerReleased;
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

        Loaded += OnLoaded;
        Unloaded += OnUnloaded;
    }

    private void OnLoaded(object sender, RoutedEventArgs e)
    {
        InitializeCoreTextEditContext();
    }

    private void OnUnloaded(object sender, RoutedEventArgs e)
    {
        _editContext?.Dispose();
        _editContext = null;
    }

    private void InitializeCoreTextEditContext()
    {
        try
        {
            var manager = CoreTextServicesManager.GetForCurrentView();
            _editContext = manager.CreateEditContext();
            _editContext.InputScope = CoreTextInputScope.Text;
            _editContext.TextRequested += OnTextRequested;
            _editContext.TextUpdating += OnTextUpdating;
            _editContext.SelectionRequested += OnSelectionRequested;
            _editContext.SelectionUpdating += OnSelectionUpdating;
            _editContext.CompositionStarted += OnCompositionStarted;
            _editContext.CompositionCompleted += OnCompositionCompleted;
            _editContext.FocusRemoved += OnFocusRemoved;
            _editContext.FormatUpdating += OnFormatUpdating;
        }
        catch { }
    }

    private void OnTextRequested(CoreTextEditContext sender, CoreTextTextRequestedEventArgs args)
    {
        args.Request.Text = GetFullText();
    }

    private void OnTextUpdating(CoreTextEditContext sender, CoreTextTextUpdatingEventArgs args)
    {
        var oldText = GetFullText();
        var newText = args.Text;

        if (oldText == newText)
        {
            args.Result = CoreTextTextUpdatingResult.Succeeded;
            return;
        }

        _suppressNotifyTextChanged = true;
        try
        {
            var startOffset = Math.Min(args.Range.StartCaretPosition, args.Range.EndCaretPosition);
            var endOffset = Math.Max(args.Range.StartCaretPosition, args.Range.EndCaretPosition);
            var replaceLength = endOffset - startOffset;
            var replacementText = newText.Substring(startOffset, newText.Length - (oldText.Length - endOffset) - startOffset);

            var result = _transactionBoundary.ReplaceRange(
                oldText, startOffset, startOffset, startOffset, replaceLength,
                replacementText, EditorTransactionCause.ImeCommit);

            LoadPlainText(result.NewText);
            var newEndOffset = args.NewSelection.EndCaretPosition;
            OffsetToLineColumn(newEndOffset, out _cursorLine, out _cursorColumn);
            ClearSelection();
            PublishText();
            TextChangedByUser?.Invoke(this, EventArgs.Empty);
            args.Result = CoreTextTextUpdatingResult.Succeeded;
        }
        catch
        {
            args.Result = CoreTextTextUpdatingResult.Failed;
        }
        finally
        {
            _suppressNotifyTextChanged = false;
        }
    }

    private void OnSelectionRequested(CoreTextEditContext sender, CoreTextSelectionRequestedEventArgs args)
    {
        var start = LineColumnToOffset(_cursorLine, _cursorColumn);
        args.Request.Selection = new CoreTextRange(start, start);
    }

    private void OnSelectionUpdating(CoreTextEditContext sender, CoreTextSelectionUpdatingEventArgs args)
    {
        var selStart = Math.Min(args.Selection.StartCaretPosition, args.Selection.EndCaretPosition);
        var selEnd = Math.Max(args.Selection.StartCaretPosition, args.Selection.EndCaretPosition);

        if (selStart == selEnd)
        {
            OffsetToLineColumn(selEnd, out _cursorLine, out _cursorColumn);
            ClearSelection();
        }
        else
        {
            OffsetToLineColumn(selStart, out _selectionStartLine, out _selectionStartColumn);
            OffsetToLineColumn(selEnd, out _selectionEndLine, out _selectionEndColumn);
            _cursorLine = _selectionEndLine;
            _cursorColumn = _selectionEndColumn;
            _hasSelection = true;
        }
        _canvas?.Invalidate();
        args.Result = CoreTextSelectionUpdatingResult.Succeeded;
    }

    private void OnCompositionStarted(CoreTextEditContext sender, CoreTextCompositionStartedEventArgs args)
    {
        _isComposing = true;
        _compositionText = string.Empty;
        _compositionCursor = 0;
    }

    private void OnCompositionCompleted(CoreTextEditContext sender, CoreTextCompositionCompletedEventArgs args)
    {
        _isComposing = false;
        _compositionText = string.Empty;
        _compositionCursor = 0;
        NotifySelectionChanged();
        _canvas?.Invalidate();
    }

    private void OnFocusRemoved(CoreTextEditContext sender, object args)
    {
        LoseFocus();
    }

    private void OnFormatUpdating(CoreTextEditContext sender, CoreTextFormatUpdatingEventArgs args)
    {
        args.Result = CoreTextFormatUpdatingResult.Succeeded;
    }

    private void NotifyTextChanged(CoreTextEditRangeChange rangeChange, int modifiedRangeStart, int modifiedRangeEnd, int newModifiedEnd)
    {
        if (_suppressNotifyTextChanged || _editContext == null) return;
        try
        {
            _editContext.NotifyTextChanged(rangeChange, modifiedRangeStart, modifiedRangeEnd,
                new CoreTextRange(newModifiedEnd, newModifiedEnd));
        }
        catch { }
    }

    private void NotifySelectionChanged()
    {
        if (_editContext == null) return;
        try
        {
            var offset = LineColumnToOffset(_cursorLine, _cursorColumn);
            _editContext.NotifySelectionChanged(new CoreTextRange(offset, offset));
        }
        catch { }
    }

    private void NotifyFocusEnter()
    {
        if (_editContext == null) return;
        try { _editContext.NotifyFocusEnter(); } catch { }
    }

    private void NotifyFocusLeave()
    {
        if (_editContext == null) return;
        try { _editContext.NotifyFocusLeave(); } catch { }
    }

    private string GetFullText()
    {
        return string.Join("\n", _lines);
    }

    private int LineColumnToOffset(int line, int column)
    {
        int offset = 0;
        for (int i = 0; i < line && i < _lines.Count; i++)
        {
            offset += _lines[i].Length + 1;
        }
        offset += Math.Min(column, _lines.Count > line ? _lines[line].Length : 0);
        return offset;
    }

    private void OffsetToLineColumn(int offset, out int line, out int column)
    {
        line = 0;
        column = 0;
        int remaining = offset;
        for (int i = 0; i < _lines.Count; i++)
        {
            if (remaining <= _lines[i].Length)
            {
                line = i;
                column = remaining;
                return;
            }
            remaining -= _lines[i].Length + 1;
        }
        line = Math.Max(0, _lines.Count - 1);
        column = _lines.Count > 0 ? _lines[_lines.Count - 1].Length : 0;
    }

    private void OnGotFocus(object sender, RoutedEventArgs e)
    {
        _cursorVisible = true;
        _cursorBlinkTimer?.Start();
        _canvas?.Invalidate();
        NotifyFocusEnter();
    }

    private void OnLostFocus(object sender, RoutedEventArgs e)
    {
        _cursorBlinkTimer?.Stop();
        _cursorVisible = false;
        _canvas?.Invalidate();
        NotifyFocusLeave();
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

    private int GetFlatCursorOffset()
    {
        int offset = 0;
        for (int i = 0; i < _cursorLine; i++)
            offset += _lines[i].Length + 1;
        offset += _cursorColumn;
        return offset;
    }

    private int GetFlatAnchorOffset()
    {
        if (!_hasSelection) return GetFlatCursorOffset();
        int offset = 0;
        int anchorLine = _selectionStartLine;
        int anchorCol = _selectionStartColumn;
        for (int i = 0; i < anchorLine; i++)
            offset += _lines[i].Length + 1;
        offset += anchorCol;
        return offset;
    }

    private int GetFlatSelectionStartOffset()
    {
        GetSelectionBounds(out int startLine, out int startCol, out _, out _);
        int offset = 0;
        for (int i = 0; i < startLine; i++)
            offset += _lines[i].Length + 1;
        offset += startCol;
        return offset;
    }

    private int GetSelectionLength()
    {
        GetSelectionBounds(out int startLine, out int startCol, out int endLine, out int endCol);
        if (startLine == endLine) return endCol - startCol;
        int len = _lines[startLine].Length - startCol;
        for (int i = startLine + 1; i < endLine; i++)
            len += _lines[i].Length + 1;
        len += endCol + 1;
        return len;
    }

    private void ApplyTransactionResult(EditorTransactionResult result)
    {
        LoadPlainText(result.NewText);
        OffsetToLineColumn(result.NewCursorOffset, out _cursorLine, out _cursorColumn);
        ClearSelection();
        PublishText();
        TextChangedByUser?.Invoke(this, EventArgs.Empty);
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

    private void CommitText(string text, EditorTransactionCause cause = EditorTransactionCause.Typing)
    {
        var oldText = string.Join('\n', _lines);
        int cursorOffset = GetFlatCursorOffset();
        int anchorOffset = GetFlatAnchorOffset();

        if (_hasSelection)
        {
            int selStart = GetFlatSelectionStartOffset();
            int selLen = GetSelectionLength();
            var result = _transactionBoundary.ReplaceRange(
                oldText, cursorOffset, anchorOffset, selStart, selLen, text, cause);
            ApplyTransactionResult(result);
        }
        else
        {
            var result = _transactionBoundary.InsertText(
                oldText, cursorOffset, anchorOffset, text, cause);
            ApplyTransactionResult(result);
        }
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
        var oldText = string.Join('\n', _lines);
        int cursorOffset = GetFlatCursorOffset();
        int anchorOffset = GetFlatAnchorOffset();

        if (_hasSelection)
        {
            int selStart = GetFlatSelectionStartOffset();
            int selLen = GetSelectionLength();
            var result = _transactionBoundary.ReplaceRange(
                oldText, cursorOffset, anchorOffset, selStart, selLen, "", EditorTransactionCause.DeleteBackward);
            ApplyTransactionResult(result);
        }
        else
        {
            var result = _transactionBoundary.DeleteBackward(
                oldText, cursorOffset, anchorOffset, EditorTransactionCause.DeleteBackward);
            ApplyTransactionResult(result);
        }
    }

    private void DeleteForward()
    {
        var oldText = string.Join('\n', _lines);
        int cursorOffset = GetFlatCursorOffset();
        int anchorOffset = GetFlatAnchorOffset();

        if (_hasSelection)
        {
            int selStart = GetFlatSelectionStartOffset();
            int selLen = GetSelectionLength();
            var result = _transactionBoundary.ReplaceRange(
                oldText, cursorOffset, anchorOffset, selStart, selLen, "", EditorTransactionCause.DeleteForward);
            ApplyTransactionResult(result);
        }
        else
        {
            var result = _transactionBoundary.DeleteForward(
                oldText, cursorOffset, anchorOffset, EditorTransactionCause.DeleteForward);
            ApplyTransactionResult(result);
        }
    }

    private void Undo()
    {
        if (_undoIndex <= 0) return;
        _undoIndex--;
        LoadPlainText(_undoStack[_undoIndex]);
        ClearSelection();
        PublishText();
    }

    private void Redo()
    {
        if (_undoIndex >= _undoStack.Count) return;
        _undoIndex++;
        LoadPlainText(_undoStack[_undoIndex - 1]);
        ClearSelection();
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

    private void ClearSelection()
    {
        _hasSelection = false;
        _selectionStartLine = _cursorLine;
        _selectionStartColumn = _cursorColumn;
        _selectionEndLine = _cursorLine;
        _selectionEndColumn = _cursorColumn;
    }

    private void SetSelectionFromDrag()
    {
        _selectionEndLine = _cursorLine;
        _selectionEndColumn = _cursorColumn;
        _hasSelection = !(_selectionStartLine == _selectionEndLine &&
                          _selectionStartColumn == _selectionEndColumn);
    }

    private void ExtendSelectionToCursor()
    {
        if (!_hasSelection)
        {
            _selectionStartLine = _cursorLine;
            _selectionStartColumn = _cursorColumn;
        }
        _selectionEndLine = _cursorLine;
        _selectionEndColumn = _cursorColumn;
        _hasSelection = !(_selectionStartLine == _selectionEndLine &&
                          _selectionStartColumn == _selectionEndColumn);
    }

    private void DeleteSelection()
    {
        if (!_hasSelection) return;

        int startLine, startCol, endLine, endCol;
        GetSelectionBounds(out startLine, out startCol, out endLine, out endCol);

        if (startLine == endLine)
        {
            var line = _lines[startLine];
            _lines[startLine] = line.Remove(startCol, endCol - startCol);
        }
        else
        {
            _lines[startLine] = _lines[startLine][..startCol] + _lines[endLine][endCol..];
            var removeCount = endLine - startLine;
            for (int i = 0; i < removeCount; i++)
                _lines.RemoveAt(startLine + 1);
        }

        _cursorLine = startLine;
        _cursorColumn = startCol;
        ClearSelection();
    }

    private void GetSelectionBounds(out int startLine, out int startCol, out int endLine, out int endCol)
    {
        if (_selectionStartLine < _selectionEndLine ||
            (_selectionStartLine == _selectionEndLine && _selectionStartColumn <= _selectionEndColumn))
        {
            startLine = _selectionStartLine;
            startCol = _selectionStartColumn;
            endLine = _selectionEndLine;
            endCol = _selectionEndColumn;
        }
        else
        {
            startLine = _selectionEndLine;
            startCol = _selectionEndColumn;
            endLine = _selectionStartLine;
            endCol = _selectionStartColumn;
        }
    }

    private string GetSelectedText()
    {
        if (!_hasSelection) return string.Empty;

        GetSelectionBounds(out int startLine, out int startCol, out int endLine, out int endCol);

        if (startLine == endLine)
        {
            return _lines[startLine][startCol..endCol];
        }

        var parts = new List<string> { _lines[startLine][startCol..] };
        for (int i = startLine + 1; i < endLine; i++)
            parts.Add(_lines[i]);
        parts.Add(_lines[endLine][..endCol]);
        return string.Join("\n", parts);
    }

    private void SelectAll()
    {
        _selectionStartLine = 0;
        _selectionStartColumn = 0;
        _selectionEndLine = _lines.Count - 1;
        _selectionEndColumn = _lines[_lines.Count - 1].Length;
        _cursorLine = _selectionEndLine;
        _cursorColumn = _selectionEndColumn;
        _hasSelection = true;
        _canvas?.Invalidate();
    }

    private async void CopyToClipboard()
    {
        var text = GetSelectedText();
        if (string.IsNullOrEmpty(text)) return;
        try
        {
            var dp = new DataPackage();
            dp.SetText(text);
            Clipboard.SetContent(dp);
        }
        catch { }
    }

    private void CutToClipboard()
    {
        if (!_hasSelection) return;
        CopyToClipboard();
        var oldText = string.Join('\n', _lines);
        int cursorOffset = GetFlatCursorOffset();
        int anchorOffset = GetFlatAnchorOffset();
        int selStart = GetFlatSelectionStartOffset();
        int selLen = GetSelectionLength();
        var result = _transactionBoundary.ReplaceRange(
            oldText, cursorOffset, anchorOffset, selStart, selLen, "", EditorTransactionCause.Cut);
        ApplyTransactionResult(result);
    }

    private async void PasteFromClipboard()
    {
        try
        {
            var content = Clipboard.GetContent();
            if (content.Contains(StandardDataFormats.Text))
            {
                var text = await content.GetTextAsync();
                if (!string.IsNullOrEmpty(text))
                {
                    CommitText(text.Replace("\r\n", "\n").Replace('\r', '\n'), EditorTransactionCause.Paste);
                }
            }
        }
        catch { }
    }

    private void OnPointerPressed(object sender, PointerRoutedEventArgs e)
    {
        Focus(FocusState.Pointer);
        var point = e.GetCurrentPoint(this);
        var indent = GetFirstLineIndent();
        var y = point.Position.Y + _scrollY;
        var newLine = Math.Clamp((int)(y / _lineHeight), 0, _lines.Count - 1);
        var line = _lines[newLine];
        int newColumn;
        if (line.Length > 0 && _textFormat != null)
        {
            var x = (float)point.Position.X - indent;
            newColumn = Math.Clamp(XToCharIndex(line, Math.Max(0, x)), 0, line.Length);
        }
        else
        {
            newColumn = 0;
        }

        if (point.Properties.IsLeftButtonPressed)
        {
            var shift = Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Shift);
            bool isShift = shift.HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);

            if (isShift)
            {
                if (!_hasSelection)
                {
                    _selectionStartLine = _cursorLine;
                    _selectionStartColumn = _cursorColumn;
                }
                _cursorLine = newLine;
                _cursorColumn = newColumn;
                _selectionEndLine = newLine;
                _selectionEndColumn = newColumn;
                _hasSelection = !(_selectionStartLine == _selectionEndLine &&
                                  _selectionStartColumn == _selectionEndColumn);
            }
            else
            {
                _cursorLine = newLine;
                _cursorColumn = newColumn;
                ClearSelection();
                _isDragging = true;
            }
        }

        _cursorVisible = true;
        _canvas?.Invalidate();
        e.Handled = true;
        NotifySelectionChanged();
    }

    private void OnPointerMoved(object sender, PointerRoutedEventArgs e)
    {
        if (!_isDragging) return;
        var point = e.GetCurrentPoint(this);
        if (!point.Properties.IsLeftButtonPressed)
        {
            _isDragging = false;
            return;
        }

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

        _selectionEndLine = _cursorLine;
        _selectionEndColumn = _cursorColumn;
        _hasSelection = !(_selectionStartLine == _selectionEndLine &&
                          _selectionStartColumn == _selectionEndColumn);

        _cursorVisible = true;
        _canvas?.Invalidate();
        e.Handled = true;
        NotifySelectionChanged();
    }

    private void OnPointerReleased(object sender, PointerRoutedEventArgs e)
    {
        _isDragging = false;
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
        var shift = Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Shift);
        bool isCtrl = ctrl.HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);
        bool isShift = shift.HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);

        if (isCtrl)
        {
            switch (e.Key)
            {
                case VirtualKey.Z:
                    if (isShift) Redo(); else Undo();
                    e.Handled = true;
                    return;
                case VirtualKey.Y:
                    Redo();
                    e.Handled = true;
                    return;
                case VirtualKey.A:
                    SelectAll();
                    e.Handled = true;
                    return;
                case VirtualKey.C:
                    CopyToClipboard();
                    e.Handled = true;
                    return;
                case VirtualKey.X:
                    CutToClipboard();
                    e.Handled = true;
                    return;
                case VirtualKey.V:
                    PasteFromClipboard();
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
                MoveCursorLeft(isShift);
                _canvas?.Invalidate();
                NotifySelectionChanged();
                e.Handled = true;
                break;
            case VirtualKey.Right:
                MoveCursorRight(isShift);
                _canvas?.Invalidate();
                NotifySelectionChanged();
                e.Handled = true;
                break;
            case VirtualKey.Up:
                MoveCursorUp(isShift);
                _canvas?.Invalidate();
                NotifySelectionChanged();
                e.Handled = true;
                break;
            case VirtualKey.Down:
                MoveCursorDown(isShift);
                _canvas?.Invalidate();
                NotifySelectionChanged();
                e.Handled = true;
                break;
            case VirtualKey.Home:
                if (isShift) ExtendSelectionToCursor();
                else ClearSelection();
                _cursorColumn = 0;
                if (isShift) { _selectionEndLine = _cursorLine; _selectionEndColumn = _cursorColumn; UpdateHasSelection(); }
                _canvas?.Invalidate();
                NotifySelectionChanged();
                e.Handled = true;
                break;
            case VirtualKey.End:
                if (isShift) ExtendSelectionToCursor();
                else ClearSelection();
                _cursorColumn = _lines[_cursorLine].Length;
                if (isShift) { _selectionEndLine = _cursorLine; _selectionEndColumn = _cursorColumn; UpdateHasSelection(); }
                _canvas?.Invalidate();
                NotifySelectionChanged();
                e.Handled = true;
                break;
            case VirtualKey.PageUp:
                if (isShift) ExtendSelectionToCursor();
                else ClearSelection();
                _cursorLine = Math.Max(0, _cursorLine - (int)(ActualHeight / _lineHeight));
                _cursorColumn = Math.Clamp(_cursorColumn, 0, _lines[_cursorLine].Length);
                if (isShift) { _selectionEndLine = _cursorLine; _selectionEndColumn = _cursorColumn; UpdateHasSelection(); }
                _canvas?.Invalidate();
                NotifySelectionChanged();
                e.Handled = true;
                break;
            case VirtualKey.PageDown:
                if (isShift) ExtendSelectionToCursor();
                else ClearSelection();
                _cursorLine = Math.Min(_lines.Count - 1, _cursorLine + (int)(ActualHeight / _lineHeight));
                _cursorColumn = Math.Clamp(_cursorColumn, 0, _lines[_cursorLine].Length);
                if (isShift) { _selectionEndLine = _cursorLine; _selectionEndColumn = _cursorColumn; UpdateHasSelection(); }
                _canvas?.Invalidate();
                NotifySelectionChanged();
                e.Handled = true;
                break;
        }
    }

    private void UpdateHasSelection()
    {
        _hasSelection = !(_selectionStartLine == _selectionEndLine &&
                          _selectionStartColumn == _selectionEndColumn);
    }

    private void MoveCursorLeft(bool isShift)
    {
        if (isShift) ExtendSelectionToCursor();
        else if (_hasSelection)
        {
            GetSelectionBounds(out _cursorLine, out _cursorColumn, out _, out _);
            ClearSelection();
            return;
        }
        else ClearSelection();

        if (_cursorColumn > 0) _cursorColumn--;
        else if (_cursorLine > 0) { _cursorLine--; _cursorColumn = _lines[_cursorLine].Length; }

        if (isShift) { _selectionEndLine = _cursorLine; _selectionEndColumn = _cursorColumn; UpdateHasSelection(); }
    }

    private void MoveCursorRight(bool isShift)
    {
        if (isShift) ExtendSelectionToCursor();
        else if (_hasSelection)
        {
            GetSelectionBounds(out _, out _, out _cursorLine, out _cursorColumn);
            ClearSelection();
            return;
        }
        else ClearSelection();

        if (_cursorColumn < _lines[_cursorLine].Length) _cursorColumn++;
        else if (_cursorLine + 1 < _lines.Count) { _cursorLine++; _cursorColumn = 0; }

        if (isShift) { _selectionEndLine = _cursorLine; _selectionEndColumn = _cursorColumn; UpdateHasSelection(); }
    }

    private void MoveCursorUp(bool isShift)
    {
        if (isShift) ExtendSelectionToCursor();
        else if (_hasSelection)
        {
            GetSelectionBounds(out _cursorLine, out _cursorColumn, out _, out _);
            ClearSelection();
        }
        else ClearSelection();

        if (_cursorLine > 0) _cursorLine--;
        _cursorColumn = Math.Clamp(_cursorColumn, 0, _lines[_cursorLine].Length);

        if (isShift) { _selectionEndLine = _cursorLine; _selectionEndColumn = _cursorColumn; UpdateHasSelection(); }
    }

    private void MoveCursorDown(bool isShift)
    {
        if (isShift) ExtendSelectionToCursor();
        else if (_hasSelection)
        {
            GetSelectionBounds(out _, out _, out _cursorLine, out _cursorColumn);
            ClearSelection();
        }
        else ClearSelection();

        if (_cursorLine + 1 < _lines.Count) _cursorLine++;
        _cursorColumn = Math.Clamp(_cursorColumn, 0, _lines[_cursorLine].Length);

        if (isShift) { _selectionEndLine = _cursorLine; _selectionEndColumn = _cursorColumn; UpdateHasSelection(); }
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
        var selectionColor = Color.FromArgb(76, 0, 120, 215);

        DrawSelection(ds, indent, contentWidth, selectionColor);

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

    private void DrawSelection(CanvasDrawingSession ds, float indent, float contentWidth, Color selectionColor)
    {
        if (!_hasSelection) return;

        GetSelectionBounds(out int startLine, out int startCol, out int endLine, out int endCol);

        for (int i = startLine; i <= endLine; i++)
        {
            if (i >= _lines.Count) break;
            var lineY = i * _lineHeight;
            if (lineY + _lineHeight < _scrollY) continue;
            if (lineY > _scrollY + ActualHeight) break;

            int lineStartCol = (i == startLine) ? startCol : 0;
            int lineEndCol = (i == endLine) ? endCol : _lines[i].Length;

            float rectX = indent;
            float rectWidth = contentWidth;

            if (_textFormat != null && _canvas?.Device != null && _lines[i].Length > 0)
            {
                try
                {
                    using var layout = new CanvasTextLayout(
                        _canvas.Device, _lines[i], _textFormat, Math.Max(1, contentWidth), _lineHeight);

                    float startX = indent;
                    float endX = indent + contentWidth;

                    if (lineStartCol > 0)
                    {
                        var startMetrics = layout.GetCaretPosition(lineStartCol, false);
                        startX = indent + startMetrics.X;
                    }
                    if (lineEndCol <= _lines[i].Length)
                    {
                        var endMetrics = layout.GetCaretPosition(lineEndCol, false);
                        endX = indent + endMetrics.X;
                    }

                    rectX = startX;
                    rectWidth = endX - startX;
                }
                catch { }
            }

            if (rectWidth > 0)
            {
                ds.FillRectangle(rectX, lineY, rectWidth, _lineHeight, selectionColor);
            }
        }
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

        if (_isComposing && !string.IsNullOrEmpty(_compositionText) && _canvas?.Device != null && _textFormat != null)
        {
            try
            {
                var contentWidth = (float)ActualWidth - indent;
                var compCursor = Math.Clamp(_compositionCursor, 0, _compositionText.Length);
                if (compCursor > 0)
                {
                    using var compLayout = new CanvasTextLayout(
                        _canvas.Device, _compositionText, _textFormat, Math.Max(1, contentWidth), _lineHeight);
                    var compMetrics = compLayout.GetCaretPosition(compCursor, false);
                    cursorX += (float)compMetrics.X;
                }
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

        if (_isComposing && !string.IsNullOrEmpty(_compositionText) && _canvas?.Device != null && _textFormat != null)
        {
            try
            {
                var contentWidth = (float)ActualWidth - indent;
                var compCursor = Math.Clamp(_compositionCursor, 0, _compositionText.Length);
                if (compCursor > 0)
                {
                    using var compLayout = new CanvasTextLayout(
                        _canvas.Device, _compositionText, _textFormat, Math.Max(1, contentWidth), _lineHeight);
                    var compMetrics = compLayout.GetCaretPosition(compCursor, false);
                    cursorX += (float)compMetrics.X;
                }
            }
            catch { }
        }

        return new Rect(cursorX, cursorY, 2, _lineHeight * 0.8);
    }

    public void StartComposition()
    {
        _isComposing = true;
        _compositionText = string.Empty;
        _compositionCursor = 0;
    }

    public void UpdateComposition(string text)
    {
        _compositionText = text;
        _compositionCursor = text.Length;
        NotifySelectionChanged();
        _canvas?.Invalidate();
    }

    public void UpdateComposition(string text, int cursor)
    {
        _compositionText = text;
        _compositionCursor = Math.Clamp(cursor, 0, text.Length);
        NotifySelectionChanged();
        _canvas?.Invalidate();
    }

    public void CommitComposition(string text)
    {
        _isComposing = false;
        _compositionText = string.Empty;
        _compositionCursor = 0;
        if (!string.IsNullOrEmpty(text))
        {
            CommitText(text);
        }
        else
        {
            NotifySelectionChanged();
            _canvas?.Invalidate();
        }
    }

    public void CancelComposition()
    {
        _isComposing = false;
        _compositionText = string.Empty;
        _compositionCursor = 0;
        NotifySelectionChanged();
        _canvas?.Invalidate();
    }

    public int GetCursorLine() => _cursorLine;
    public int GetCursorColumn() => _cursorColumn;
    public int GetLineCount() => _lines.Count;
    public bool HasSelection => _hasSelection;
    public string SelectedText => GetSelectedText();
}
