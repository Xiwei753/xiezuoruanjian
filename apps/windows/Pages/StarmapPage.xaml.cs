using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Sujian.Windows.Bridge;
using System.Collections.ObjectModel;

namespace Sujian.Windows.Pages;

public sealed partial class StarmapPage : Page
{
    private readonly WriterCoreBridge _core = new();
    private string? _projectId;
    private readonly ObservableCollection<StarmapSummaryDto> _starmaps = new();

    public StarmapPage()
    {
        InitializeComponent();
        StarmapList.ItemsSource = _starmaps;
    }

    public void SetProject(string projectId)
    {
        _projectId = projectId;
        LoadStarmaps();
    }

    private async void LoadStarmaps()
    {
        if (_projectId == null) return;
        try
        {
            var list = await _core.ListStarmapsAsync(_projectId);
            _starmaps.Clear();
            foreach (var sm in list) _starmaps.Add(sm);
            EmptyText.Visibility = _starmaps.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        }
        catch (WriterCoreException ex)
        {
            StatusText.Text = $"加载星图失败: {ex.Code}";
        }
    }

    private async void CreateStarmap_Click(object sender, RoutedEventArgs e)
    {
        if (_projectId == null) return;
        var dialog = new ContentDialog
        {
            Title = "新建星图",
            PrimaryButtonText = "创建",
            CloseButtonText = "取消",
            Content = new TextBox { PlaceholderText = "星图名称" }
        };
        dialog.XamlRoot = Content.XamlRoot;
        var result = await dialog.ShowAsync();
        if (result == ContentDialogResult.Primary && dialog.Content is TextBox tb && !string.IsNullOrWhiteSpace(tb.Text))
        {
            try
            {
                await _core.CreateStarmapAsync(_projectId, tb.Text.Trim());
                LoadStarmaps();
            }
            catch (WriterCoreException ex)
            {
                StatusText.Text = $"创建星图失败: {ex.Code}";
            }
        }
    }

    private void Refresh_Click(object sender, RoutedEventArgs e)
    {
        LoadStarmaps();
    }

    private void StarmapList_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (StarmapList.SelectedItem is StarmapSummaryDto sm)
        {
            System.Diagnostics.Debug.WriteLine($"[StarmapPage] Selected: {sm.Name} ({sm.Id})");
        }
    }
}
