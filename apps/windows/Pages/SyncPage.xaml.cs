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

    private static string SyncErrorToUserMessage(string errorCode, string? rawMessage)
    {
        return errorCode switch
        {
            "token_missing" => "未设置 Token",
            "token_invalid" => "GitHub Token 无效或已过期。请检查 token 是否正确。",
            "token_permission_denied" => "GitHub Token 权限不足。请给该 token 勾选目标仓库，并授予 Contents: Read and write。",
            "repo_not_found_or_no_permission" => "仓库不存在或 Token 无权限",
            "auth_failed" => "认证失败",
            "network_failed" => "网络连接失败",
            "branch_missing" => "远程分支不存在",
            "non_fast_forward" => "远端有更新，请先拉取",
            "unrelated_histories" => "本地与远端历史不相关",
            "conflict" => "同步冲突，请手动处理冲突文件后重试",
            "not_configured" => "同步未配置",
            _ => "同步失败，请检查网络和配置"
        };
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
            StatusText.Text = $"加载同步配置失败: {SyncErrorToUserMessage(ex.Code, ex.Message)}";
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
            StatusText.Text = "配置已保存";
        }
        catch (WriterCoreException ex)
        {
            StatusText.Text = $"保存配置失败: {SyncErrorToUserMessage(ex.Code, ex.Message)}";
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
            StatusText.Text = "预演完成";
        }
        catch (WriterCoreException ex)
        {
            StatusText.Text = $"试运行失败: {SyncErrorToUserMessage(ex.Code, ex.Message)}";
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
            StatusText.Text = SyncErrorToUserMessage(ex.Code, ex.Message);
        }
    }
}
