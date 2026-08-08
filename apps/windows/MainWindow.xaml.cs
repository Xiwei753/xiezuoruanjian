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
    private Editor.SujianEditorHost? _currentEditor;

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
            var projects = await _core.OpenDataRootAsync(null);
            _projects.Clear();
            _projects.AddRange(projects);
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
            ShowError($"打开数据目录失败: {ex.Message}");
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
        if (result == ContentDialogResult.Primary && dialog.Content is TextBox tb && !string.IsNullOrWhiteSpace(tb.Text))
        {
            try
            {
                await _core.CreateProjectAsync(tb.Text.Trim());
                var projects = await _core.OpenDataRootAsync(null);
                _projects.Clear();
                _projects.AddRange(projects);
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
        if (result == ContentDialogResult.Primary && dialog.Content is TextBox tb && !string.IsNullOrWhiteSpace(tb.Text))
        {
            try
            {
                await _core.CreateChapterAsync(_currentProjectId, _currentVolumeId, tb.Text.Trim());
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
            switch (tag)
            {
                case "projects":
                    if (_currentProjectId != null) OpenChapterInEditor();
                    break;
                case "stats":
                    ContentFrame.Navigate(typeof(StatsPage));
                    break;
                case "starmap":
                    var starmapPage = new StarmapPage();
                    if (_currentProjectId != null) starmapPage.SetProject(_currentProjectId);
                    ContentFrame.Content = starmapPage;
                    break;
                case "sync":
                    ContentFrame.Navigate(typeof(SyncPage));
                    break;
            }
        }
        else if (args.IsSettingsSelected)
        {
            ContentFrame.Navigate(typeof(SettingsPage));
        }
    }

    private void RefreshNav()
    {
        NavView.MenuItems.Clear();
        NavView.MenuItems.Add(new NavigationViewItem { Content = "项目", Tag = "projects", Icon = new SymbolIcon(Symbol.Library) });
        NavView.MenuItems.Add(new NavigationViewItem { Content = "写作统计", Tag = "stats", Icon = new SymbolIcon(Symbol.Repair) });
        NavView.MenuItems.Add(new NavigationViewItem { Content = "星图", Tag = "starmap", Icon = new SymbolIcon(Symbol.Map) });
        NavView.MenuItems.Add(new NavigationViewItem { Content = "同步", Tag = "sync", Icon = new SymbolIcon(Symbol.Sync) });

        foreach (var p in _projects)
        {
            var projectItem = new NavigationViewItem { Content = p.Name, Tag = $"project:{p.Id}" };
            foreach (var v in _volumes.Where(v => true))
            {
                projectItem.MenuItems.Add(new NavigationViewItem { Content = v.Name, Tag = $"volume:{v.Id}" });
            }
            NavView.MenuItems.Add(projectItem);
        }
    }

    private async void OpenChapterInEditor()
    {
        if (_currentProjectId is null || _currentVolumeId is null || _currentChapterId is null) return;

        try
        {
            var content = await _core.OpenChapterAsync(_currentProjectId, _currentVolumeId, _currentChapterId);
            _currentEditor = new Editor.SujianEditorHost { Text = content };
            _currentEditor.SetChapterContext(_currentProjectId, _currentVolumeId, _currentChapterId, _core);
            _currentEditor.EnableAutoSave();
            ContentFrame.Content = _currentEditor;
        }
        catch (WriterCoreException ex)
        {
            ShowError($"打开章节失败: {ex.Message}");
        }
    }

    private Editor.SujianEditorHost? GetActiveEditorHost()
    {
        return _currentEditor;
    }

    private Editor.SujianEditor? GetActiveEditor()
    {
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
