using Microsoft.UI.Xaml;
using Sujian.Windows.Bridge;

namespace Sujian.Windows;

public sealed partial class MainWindow : Window
{
    private readonly WriterCoreBridge _core = new();
    private string? _currentChapterId;

    public MainWindow()
    {
        InitializeComponent();
        Title = "素笺 Windows 原生客户端";
        Editor.Text = "";
    }

    private async void OpenWorkspace_Click(object sender, RoutedEventArgs e)
    {
        var workspace = await _core.OpenWorkspaceAsync(null);
        var firstProject = workspace.Projects.FirstOrDefault();
        var firstVolume = firstProject is null ? null : (await _core.ListVolumesAsync(firstProject.Id)).FirstOrDefault();
        var firstChapter = firstVolume is null ? null : (await _core.ListChaptersAsync(firstProject!.Id, firstVolume.Id)).FirstOrDefault();
        if (firstProject is not null && firstVolume is not null && firstChapter is not null)
        {
            _currentChapterId = firstChapter.Id;
            Editor.Text = await _core.OpenChapterAsync(firstProject.Id, firstVolume.Id, firstChapter.Id);
        }
    }

    private async void SaveChapter_Click(object sender, RoutedEventArgs e)
    {
        if (_currentChapterId is null) return;
        await _core.SaveChapterAsync(_currentChapterId, Editor.Text);
    }
}
