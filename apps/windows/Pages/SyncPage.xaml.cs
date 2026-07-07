using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Sujian.Windows.Bridge;

namespace Sujian.Windows.Pages;

public sealed partial class SyncPage : Page
{
    private readonly WriterCoreBridge _core = new();

    public SyncPage()
    {
        InitializeComponent();
        LoadConfig();
    }

    private async void LoadConfig()
    {
        try
        {
            var config = await _core.LoadSyncConfigAsync();
            RemoteUrlBox.Text = config.RemoteUrl;
            AccessTokenBox.Password = config.AccessToken;
            AutoSyncToggle.IsOn = config.AutoSync;
            SyncIntervalBox.Value = config.IntervalMinutes;
        }
        catch (WriterCoreException ex)
        {
            StatusText.Text = $"加载同步配置失败: {ex.Code}";
        }
    }

    private async void SaveConfig_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            var config = new SyncConfigDto(
                RemoteUrlBox.Text,
                AccessTokenBox.Password,
                AutoSyncToggle.IsOn,
                (int)SyncIntervalBox.Value
            );
            await _core.SaveSyncConfigAsync(config);
            StatusText.Text = "同步配置已保存";
        }
        catch (WriterCoreException ex)
        {
            StatusText.Text = $"保存失败: {ex.Code}";
        }
    }

    private async void DryRun_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            StatusText.Text = "正在试运行...";
            var result = await _core.SyncDryRunAsync();
            ResultBox.Visibility = Visibility.Visible;
            ResultBox.Text = result;
            StatusText.Text = "试运行完成";
        }
        catch (WriterCoreException ex)
        {
            StatusText.Text = $"试运行失败: {ex.Code}";
        }
    }

    private async void SyncNow_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            StatusText.Text = "正在同步...";
            var result = await _core.PerformSyncAsync();
            ResultBox.Visibility = Visibility.Visible;
            ResultBox.Text = result;
            StatusText.Text = "同步完成";
        }
        catch (WriterCoreException ex)
        {
            StatusText.Text = $"同步失败: {ex.Code}";
        }
    }
}
