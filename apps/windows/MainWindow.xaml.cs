using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Sujian.Windows.Bridge;
using Sujian.Windows.Pages;
using System;
using System.Collections.Generic;
using System.Linq;

namespace Sujian.Windows;

public sealed partial class MainWindow : Window
{
    private readonly WriterCoreBridge _core = new();
    private string? _currentProjectId;
    private string? _currentVolumeId;
    private string? _currentChapterId;
    private readonly List<ProjectSummary> _projects = new();
    private readonly List<VolumeSummary> _volumes = new();
    private readonly List<ChapterSummary> _chapters = new();

    public MainWindow()
    {
        InitializeComponent();
        Title = "素笺 Windows 原生客户端";
        ExtendsContentIntoTitleBar = true;

        var appWindow = this.AppWindow;
        appWindow.Resize(new Windows.Graphics.SizeInt32(1200, 800));
    }

    private async void OpenWorkspace_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            var workspace = await _core.OpenWorkspaceAsync(null);
            _projects.Clear();
            _projects.AddRange(workspace.Projects);
            RefreshNav();

            if (_projects.Count > 0)
            {
                _currentProjectId = _projects[0].Id;
                var volumes = await _core.ListVolumesAsync(_currentProjectId);
                _volumes.Clear();
                _volumes.AddRange(volumes);
                if (_volumes.Count > 0)
                {
                    _currentVolumeId = _volumes[0].Id;
                    var chapters = await _core.ListChaptersAsync(_currentProjectId, _currentVolumeId);
                    _chapters.Clear();
                    _chapters.AddRange(chapters);
                    if (_chapters.Count > 0)
                    {
                        _currentChapterId = _chapters[0].Id;
                        OpenChapterInEditor();
                    }
                }
            }
        }
        catch (WriterCoreException ex)
        {
            ShowError($"打开工作区失败: {ex.Message}");
        }
    }

    private async void SaveChapter_Click(object sender, RoutedEventArgs e)
    {
        if (_currentProjectId is null || _currentVolumeId is null || _currentChapterId is null)
        {
            ShowError("请先打开一个章节");
            return;
        }

        try
        {
            var editor = GetActiveEditor();
            if (editor is null) return;
            await _core.SaveChapterAsync(_currentProjectId, _currentVolumeId, _currentChapterId, editor.Text);
        }
        catch (WriterCoreException ex)
        {
            ShowError($"保存失败: {ex.Message}");
        }
    }

    private async void CreateProject_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new ContentDialog
        {
            Title = "新建项目",
            PrimaryButtonText = "创建",
            CloseButtonText = "取消",
            Content = new TextBox { PlaceholderText = "项目名称" }
        };
        dialog.XamlRoot = Content.XamlRoot;
        var result = await dialog.ShowAsync();
        if (result == ContentDialogResult.Primary && dialog.Content is TextBox tb)
        {
            try
            {
                var workspace = await _core.OpenWorkspaceAsync(null);
                _projects.Clear();
                _projects.AddRange(workspace.Projects);
                RefreshNav();
            }
            catch (WriterCoreException ex)
            {
                ShowError($"创建项目失败: {ex.Message}");
            }
        }
    }

    private async void CreateChapter_Click(object sender, RoutedEventArgs e)
    {
        if (_currentProjectId is null || _currentVolumeId is null)
        {
            ShowError("请先选择卷");
            return;
        }

        var dialog = new ContentDialog
        {
            Title = "新建章节",
            PrimaryButtonText = "创建",
            CloseButtonText = "取消",
            Content = new TextBox { PlaceholderText = "章节标题" }
        };
        dialog.XamlRoot = Content.XamlRoot;
        var result = await dialog.ShowAsync();
        if (result == ContentDialogResult.Primary && dialog.Content is TextBox tb)
        {
            try
            {
                var chapters = await _core.ListChaptersAsync(_currentProjectId, _currentVolumeId);
                _chapters.Clear();
                _chapters.AddRange(chapters);
                RefreshNav();
            }
            catch (WriterCoreException ex)
            {
                ShowError($"创建章节失败: {ex.Message}");
            }
        }
    }

    private void Settings_Click(object sender, RoutedEventArgs e)
    {
        ContentFrame.Navigate(typeof(SettingsPage));
    }

    private void Nav_SelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (args.SelectedItem is NavigationViewItem item && item.Tag is string tag)
        {
            if (tag == "projects" && _currentProjectId != null)
            {
                OpenChapterInEditor();
            }
        }
    }

    private void RefreshNav()
    {
        NavView.MenuItems.Clear();
        foreach (var p in _projects)
        {
            NavView.MenuItems.Add(new NavigationViewItem { Content = p.Name, Tag = p.Id });
        }
    }

    private async void OpenChapterInEditor()
    {
        if (_currentProjectId is null || _currentVolumeId is null || _currentChapterId is null) return;

        try
        {
            var content = await _core.OpenChapterAsync(_currentProjectId, _currentVolumeId, _currentChapterId);
            var editor = new Editor.SujianEditor { Text = content };
            ContentFrame.Content = editor;
        }
        catch (WriterCoreException ex)
        {
            ShowError($"打开章节失败: {ex.Message}");
        }
    }

    private Editor.SujianEditor? GetActiveEditor()
    {
        if (ContentFrame.Content is Editor.SujianEditor editor) return editor;
        return null;
    }

    private async void ShowError(string message)
    {
        var dialog = new ContentDialog
        {
            Title = "错误",
            CloseButtonText = "确定",
            Content = message
        };
        dialog.XamlRoot = Content.XamlRoot;
        await dialog.ShowAsync();
    }
}
