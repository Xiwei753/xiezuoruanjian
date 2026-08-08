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

    private static string ResolveUserMessage(WriterCoreException ex)
    {
        if (!string.IsNullOrEmpty(ex.MessageKey))
        {
            return MessageKeyToChinese(ex.MessageKey, ex.MessageArgs);
        }
        return ErrorCodeFallbackToChinese(ex.Code);
    }

    private static string MessageKeyToChinese(string messageKey, Dictionary<string, string>? args)
    {
        return messageKey switch
        {
            "error.sync_conflict" => args != null && args.TryGetValue("detail", out var d) && d.Contains("path conflict")
                ? "同步冲突，请手动处理冲突文件后重试"
                : "同步冲突，请手动处理冲突文件后重试",
            "error.sync_failed" => args != null && args.TryGetValue("detail", out var detail)
                ? SyncDetailToChinese(detail)
                : "同步失败，请检查网络和配置",
            "error.io" => "读写错误，请检查磁盘空间",
            "error.json" => "数据格式错误",
            "error.project_not_found" => "项目未找到",
            "error.volume_not_found" => "卷未找到",
            "error.chapter_not_found" => "章节未找到",
            "error.other" => "操作失败，请重试",
            "sync.block.disabled" => "同步已禁用",
            "sync.block.remote_url_missing" => "未设置远程仓库地址",
            "sync.block.token_missing" => "未设置 Token",
            _ => $"操作失败 ({messageKey})"
        };
    }

    private static string SyncDetailToChinese(string detail)
    {
        var d = detail.ToLowerInvariant();
        if (d.Contains("auth") || d.Contains("token") || d.Contains("credential"))
            return "认证失败，请检查 GitHub Token";
        if (d.Contains("network") || d.Contains("timeout") || d.Contains("connect") || d.Contains("resolve"))
            return "网络连接失败，请检查网络";
        if (d.Contains("not_found") || d.Contains("404"))
            return "仓库不存在或 Token 无权限";
        if (d.Contains("non_fast_forward") || d.Contains("unrelated"))
            return "远端有更新或历史不相关，请先拉取";
        return "同步失败，请检查网络和配置";
    }

    private static string ErrorCodeFallbackToChinese(string errorCode)
    {
        return errorCode switch
        {
            "token_missing" => "未设置 Token",
            "token_invalid" => "GitHub Token 无效或已过期",
            "token_permission_denied" => "GitHub Token 权限不足",
            "repo_not_found_or_no_permission" => "仓库不存在或 Token 无权限",
            "auth_failed" => "认证失败",
            "network_failed" => "网络连接失败",
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
            StatusText.Text = $"加载同步配置失败: {ResolveUserMessage(ex)}";
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
            StatusText.Text = $"保存配置失败: {ResolveUserMessage(ex)}";
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
            StatusText.Text = $"试运行失败: {ResolveUserMessage(ex)}";
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
            StatusText.Text = ResolveUserMessage(ex);
        }
    }
}
