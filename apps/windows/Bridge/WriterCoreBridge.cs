using System.Runtime.InteropServices;
using System.Text.Json;

namespace Sujian.Windows.Bridge;

public sealed record WorkspaceSummary(IReadOnlyList<ProjectSummary> Projects);
public sealed record ProjectSummary(string Id, string Name);
public sealed record VolumeSummary(string Id, string Name);
public sealed record ChapterSummary(string Id, string Title);
public sealed record SyncConfigDto(string RemoteUrl, string AccessToken, bool AutoSync, int IntervalMinutes);
public sealed record WritingStatsDto(int TotalWords, int TodayWords, int SessionWords, int StreakDays, string SessionStartTime);
public sealed record StarmapSummaryDto(string Id, string Name, int NodeCount, int EdgeCount);
public sealed record ProjectStatsDto(int TotalWords, int TotalChapters, string LastEditedAt);

/// Bridge 信封结果 — 与 Core ResultEnvelope 对齐的统一响应结构。
/// ErrorCode 与 Core Error.code() 返回的字符串一致，是跨端 API 契约。
internal sealed class EnvelopeResult
{
    public bool Success { get; set; }
    public JsonElement? Data { get; set; }
    public string? ErrorCode { get; set; }
    public string? MessageKey { get; set; }
    public Dictionary<string, string>? MessageArgs { get; set; }
    public string? UserMessage { get; set; }
    public string? RawError { get; set; }
}

/// Windows 端 Core FFI Bridge — 通过 P/Invoke 调用 writer_core 原生库。
///
/// 所有 FFI 函数返回 JSON 字符串（IntPtr），由 CallFFI 统一解析为 EnvelopeResult。
/// 字符串指针必须通过 writer_core_free_string 释放，避免内存泄漏。
/// 调用方通过 WrapResult 将 EnvelopeResult 转换为业务类型或抛出异常。
public sealed class WriterCoreBridge
{
    private const string DllName = "writer_core";

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern int writer_core_init(IntPtr path);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_open_workspace(IntPtr path);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_list_projects();

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_list_volumes(IntPtr projectId);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_list_chapters(IntPtr projectId, IntPtr volumeId);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_open_chapter(IntPtr projectId, IntPtr volumeId, IntPtr chapterId);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_save_chapter(IntPtr projectId, IntPtr volumeId, IntPtr chapterId, IntPtr content);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_load_local_settings();

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_save_local_settings(IntPtr settingsJson);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_list_palette_records();

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_load_palette_record(IntPtr deviceId, IntPtr fingerprint);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_delete_palette_record(IntPtr deviceId, IntPtr fingerprint);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_list_builtin_themes();

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern int writer_core_calculate_word_count(IntPtr text);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern void writer_core_free_string(IntPtr ptr);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_get_last_error();

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_create_project(IntPtr name);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_create_volume(IntPtr projectId, IntPtr name);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_create_chapter(IntPtr projectId, IntPtr volumeId, IntPtr title);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_get_project_stats(IntPtr projectId);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_load_sync_config();

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_save_sync_config(IntPtr configJson);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_perform_sync();

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_sync_dry_run();

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_get_writing_stats();

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_list_starmaps_for_project(IntPtr projectId);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_get_starmap(IntPtr starmapId);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_create_starmap(IntPtr projectId, IntPtr name);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_editor_visual_transaction(
        IntPtr oldText, IntPtr newText, uint oldCursorIndex, uint newCursorIndex,
        IntPtr cause, uint maxAnimatedChars, uint animationDurationMs);

    private static IntPtr ToUtf8(string? s)
    {
        if (s is null) return IntPtr.Zero;
        var bytes = System.Text.Encoding.UTF8.GetBytes(s + "\0");
        var ptr = Marshal.AllocHGlobal(bytes.Length);
        Marshal.Copy(bytes, 0, ptr, bytes.Length);
        return ptr;
    }

    private static string? PtrToStringAndFree(IntPtr ptr)
    {
        if (ptr == IntPtr.Zero) return null;
        var str = Marshal.PtrToStringUTF8(ptr);
        writer_core_free_string(ptr);
        return str;
    }

    private static EnvelopeResult ParseEnvelope(string? json)
    {
        if (json is null) return new EnvelopeResult { Success = false, ErrorCode = "NULL_RESPONSE" };
        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;

        var result = new EnvelopeResult
        {
            Success = root.TryGetProperty("success", out var succ) && succ.GetBoolean(),
            ErrorCode = root.TryGetProperty("errorCode", out var ec) ? ec.GetString() : null,
            MessageKey = root.TryGetProperty("messageKey", out var mk) ? mk.GetString() : null,
            RawError = root.TryGetProperty("rawError", out var re) ? re.GetString() : null,
        };

        if (root.TryGetProperty("messageArgs", out var ma) && ma.ValueKind == JsonValueKind.Object)
        {
            result.MessageArgs = new Dictionary<string, string>();
            foreach (var prop in ma.EnumerateObject())
            {
                result.MessageArgs[prop.Name] = prop.Value.GetString() ?? "";
            }
        }

        if (root.TryGetProperty("data", out var data) && data.ValueKind != JsonValueKind.Null)
        {
            result.Data = data.Clone();
        }

        return result;
    }

    private static void ThrowIfFailed(EnvelopeResult env)
    {
        if (!env.Success)
            throw new WriterCoreException(
                env.ErrorCode ?? "UNKNOWN",
                env.MessageKey,
                env.MessageArgs,
                env.UserMessage ?? env.RawError ?? "Unknown error");
    }

    public int InitWorkspace(string path)
    {
        var pathPtr = ToUtf8(path);
        try
        {
            return writer_core_init(pathPtr);
        }
        finally
        {
            Marshal.FreeHGlobal(pathPtr);
        }
    }

    public Task<WorkspaceSummary> OpenWorkspaceAsync(string? path)
    {
        var pathPtr = ToUtf8(path);
        try
        {
            var resultPtr = writer_core_open_workspace(pathPtr);
            var json = PtrToStringAndFree(resultPtr);
            var env = ParseEnvelope(json);
            ThrowIfFailed(env);

            var projects = new List<ProjectSummary>();
            if (env.Data?.ValueKind == JsonValueKind.Object && env.Data.Value.TryGetProperty("projects", out var arr))
            {
                foreach (var p in arr.EnumerateArray())
                {
                    projects.Add(new ProjectSummary(
                        p.GetProperty("id").GetString() ?? "",
                        p.GetProperty("title").GetString() ?? ""
                    ));
                }
            }
            return Task.FromResult(new WorkspaceSummary(projects));
        }
        finally
        {
            Marshal.FreeHGlobal(pathPtr);
        }
    }

    public Task<IReadOnlyList<ProjectSummary>> ListProjectsAsync()
    {
        var resultPtr = writer_core_list_projects();
        var json = PtrToStringAndFree(resultPtr);
        var env = ParseEnvelope(json);
        ThrowIfFailed(env);

        var list = new List<ProjectSummary>();
        if (env.Data?.ValueKind == JsonValueKind.Array)
        {
            foreach (var p in env.Data.Value.EnumerateArray())
            {
                list.Add(new ProjectSummary(
                    p.GetProperty("id").GetString() ?? "",
                    p.GetProperty("title").GetString() ?? ""
                ));
            }
        }
        return Task.FromResult<IReadOnlyList<ProjectSummary>>(list);
    }

    public Task<IReadOnlyList<VolumeSummary>> ListVolumesAsync(string projectId)
    {
        var pidPtr = ToUtf8(projectId);
        try
        {
            var resultPtr = writer_core_list_volumes(pidPtr);
            var json = PtrToStringAndFree(resultPtr);
            var env = ParseEnvelope(json);
            ThrowIfFailed(env);

            var list = new List<VolumeSummary>();
            if (env.Data?.ValueKind == JsonValueKind.Array)
            {
                foreach (var v in env.Data.Value.EnumerateArray())
                {
                    list.Add(new VolumeSummary(
                        v.GetProperty("id").GetString() ?? "",
                        v.GetProperty("title").GetString() ?? ""
                    ));
                }
            }
            return Task.FromResult<IReadOnlyList<VolumeSummary>>(list);
        }
        finally
        {
            Marshal.FreeHGlobal(pidPtr);
        }
    }

    public Task<IReadOnlyList<ChapterSummary>> ListChaptersAsync(string projectId, string volumeId)
    {
        var pidPtr = ToUtf8(projectId);
        var vidPtr = ToUtf8(volumeId);
        try
        {
            var resultPtr = writer_core_list_chapters(pidPtr, vidPtr);
            var json = PtrToStringAndFree(resultPtr);
            var env = ParseEnvelope(json);
            ThrowIfFailed(env);

            var list = new List<ChapterSummary>();
            if (env.Data?.ValueKind == JsonValueKind.Array)
            {
                foreach (var c in env.Data.Value.EnumerateArray())
                {
                    list.Add(new ChapterSummary(
                        c.GetProperty("id").GetString() ?? "",
                        c.GetProperty("title").GetString() ?? ""
                    ));
                }
            }
            return Task.FromResult<IReadOnlyList<ChapterSummary>>(list);
        }
        finally
        {
            Marshal.FreeHGlobal(pidPtr);
            Marshal.FreeHGlobal(vidPtr);
        }
    }

    public Task<string> OpenChapterAsync(string projectId, string volumeId, string chapterId)
    {
        var pidPtr = ToUtf8(projectId);
        var vidPtr = ToUtf8(volumeId);
        var cidPtr = ToUtf8(chapterId);
        try
        {
            var resultPtr = writer_core_open_chapter(pidPtr, vidPtr, cidPtr);
            var json = PtrToStringAndFree(resultPtr);
            var env = ParseEnvelope(json);
            ThrowIfFailed(env);

            var content = env.Data?.GetProperty("content").GetString() ?? string.Empty;
            return Task.FromResult(content);
        }
        finally
        {
            Marshal.FreeHGlobal(pidPtr);
            Marshal.FreeHGlobal(vidPtr);
            Marshal.FreeHGlobal(cidPtr);
        }
    }

    public Task SaveChapterAsync(string projectId, string volumeId, string chapterId, string plainText)
    {
        var pidPtr = ToUtf8(projectId);
        var vidPtr = ToUtf8(volumeId);
        var cidPtr = ToUtf8(chapterId);
        var contentPtr = ToUtf8(plainText);
        try
        {
            var resultPtr = writer_core_save_chapter(pidPtr, vidPtr, cidPtr, contentPtr);
            var json = PtrToStringAndFree(resultPtr);
            var env = ParseEnvelope(json);
            ThrowIfFailed(env);
            return Task.CompletedTask;
        }
        finally
        {
            Marshal.FreeHGlobal(pidPtr);
            Marshal.FreeHGlobal(vidPtr);
            Marshal.FreeHGlobal(cidPtr);
            Marshal.FreeHGlobal(contentPtr);
        }
    }

    public Task<ProjectSummary> CreateProjectAsync(string name)
    {
        var namePtr = ToUtf8(name);
        try
        {
            var resultPtr = writer_core_create_project(namePtr);
            var json = PtrToStringAndFree(resultPtr);
            var env = ParseEnvelope(json);
            ThrowIfFailed(env);

        var id = env.Data?.GetProperty("id").GetString() ?? "";
        var title = env.Data?.GetProperty("title").GetString() ?? name;
        return Task.FromResult(new ProjectSummary(id, title));
        }
        finally
        {
            Marshal.FreeHGlobal(namePtr);
        }
    }

    public Task<VolumeSummary> CreateVolumeAsync(string projectId, string name)
    {
        var pidPtr = ToUtf8(projectId);
        var namePtr = ToUtf8(name);
        try
        {
            var resultPtr = writer_core_create_volume(pidPtr, namePtr);
            var json = PtrToStringAndFree(resultPtr);
            var env = ParseEnvelope(json);
            ThrowIfFailed(env);

            var id = env.Data?.GetProperty("id").GetString() ?? "";
            var title = env.Data?.GetProperty("title").GetString() ?? name;
            return Task.FromResult(new VolumeSummary(id, title));
        }
        finally
        {
            Marshal.FreeHGlobal(pidPtr);
            Marshal.FreeHGlobal(namePtr);
        }
    }

    public Task<ChapterSummary> CreateChapterAsync(string projectId, string volumeId, string title)
    {
        var pidPtr = ToUtf8(projectId);
        var vidPtr = ToUtf8(volumeId);
        var titlePtr = ToUtf8(title);
        try
        {
            var resultPtr = writer_core_create_chapter(pidPtr, vidPtr, titlePtr);
            var json = PtrToStringAndFree(resultPtr);
            var env = ParseEnvelope(json);
            ThrowIfFailed(env);

            var id = env.Data?.GetProperty("id").GetString() ?? "";
            var chTitle = env.Data?.GetProperty("title").GetString() ?? title;
            return Task.FromResult(new ChapterSummary(id, chTitle));
        }
        finally
        {
            Marshal.FreeHGlobal(pidPtr);
            Marshal.FreeHGlobal(vidPtr);
            Marshal.FreeHGlobal(titlePtr);
        }
    }

    public Task<ProjectStatsDto> GetProjectStatsAsync(string projectId)
    {
        var pidPtr = ToUtf8(projectId);
        try
        {
            var resultPtr = writer_core_get_project_stats(pidPtr);
            var json = PtrToStringAndFree(resultPtr);
            var env = ParseEnvelope(json);
            ThrowIfFailed(env);

            int totalWords = 0, totalChapters = 0;
            string lastEditedAt = "";
            if (env.Data?.ValueKind == JsonValueKind.Object)
            {
                if (env.Data.Value.TryGetProperty("totalWords", out var tw)) totalWords = tw.GetInt32();
                if (env.Data.Value.TryGetProperty("totalChapters", out var tc)) totalChapters = tc.GetInt32();
                if (env.Data.Value.TryGetProperty("lastEditedAt", out var lea)) lastEditedAt = lea.GetString() ?? "";
            }
            return Task.FromResult(new ProjectStatsDto(totalWords, totalChapters, lastEditedAt));
        }
        finally
        {
            Marshal.FreeHGlobal(pidPtr);
        }
    }

    public Task<LocalSettings> LoadSettingsAsync()
    {
        var resultPtr = writer_core_load_local_settings();
        var json = PtrToStringAndFree(resultPtr);
        var env = ParseEnvelope(json);
        ThrowIfFailed(env);

        var s = new LocalSettings();
        if (env.Data?.ValueKind == JsonValueKind.Object)
        {
            if (env.Data.Value.TryGetProperty("fontSize", out var fs)) s.FontSize = (float)fs.GetDouble();
            if (env.Data.Value.TryGetProperty("lineHeight", out var lh)) s.LineHeight = (float)lh.GetDouble();
            if (env.Data.Value.TryGetProperty("theme", out var th)) s.Theme = th.GetString() ?? "system";
            if (env.Data.Value.TryGetProperty("appearanceMode", out var am)) s.AppearanceMode = am.GetString() ?? "system";
            if (env.Data.Value.TryGetProperty("colorSource", out var cs)) s.ColorSource = cs.GetString() ?? "built_in";
            if (env.Data.Value.TryGetProperty("dynamicColorEnabled", out var dce)) s.DynamicColorEnabled = dce.GetBoolean();
            if (env.Data.Value.TryGetProperty("selectedBuiltinThemeId", out var sbti)) s.SelectedBuiltinThemeId = sbti.GetString() ?? "";
            if (env.Data.Value.TryGetProperty("selectedPaletteId", out var spi)) s.SelectedPaletteId = spi.GetString() ?? "";
            if (env.Data.Value.TryGetProperty("autoSave", out var asv)) s.AutoSave = asv.GetBoolean();
            if (env.Data.Value.TryGetProperty("autoIndent", out var ai)) s.AutoIndent = ai.GetBoolean();
            if (env.Data.Value.TryGetProperty("typingAnimationEnabled", out var tae)) s.TypingAnimationEnabled = tae.GetBoolean();
            if (env.Data.Value.TryGetProperty("typingAnimationDurationMs", out var tadm)) s.TypingAnimationDurationMs = tadm.GetInt32();
            if (env.Data.Value.TryGetProperty("coordinatedTextCursorAnimationEnabled", out var cca)) s.CoordinatedTextCursorAnimationEnabled = cca.GetBoolean();
            if (env.Data.Value.TryGetProperty("smoothCursorEnabled", out var sce)) s.SmoothCursorEnabled = sce.GetBoolean();
            if (env.Data.Value.TryGetProperty("smoothCursorDurationMs", out var scdm)) s.SmoothCursorDurationMs = scdm.GetInt32();
        }
        return Task.FromResult(s);
    }

    public Task SaveSettingsAsync(LocalSettings settings)
    {
        var json = JsonSerializer.Serialize(new
        {
            fontSize = settings.FontSize,
            lineHeight = settings.LineHeight,
            theme = settings.Theme,
            appearanceMode = settings.AppearanceMode,
            colorSource = settings.ColorSource,
            dynamicColorEnabled = settings.DynamicColorEnabled,
            selectedBuiltinThemeId = settings.SelectedBuiltinThemeId,
            selectedPaletteId = settings.SelectedPaletteId,
            autoSave = settings.AutoSave,
            autoIndent = settings.AutoIndent,
            typingAnimationEnabled = settings.TypingAnimationEnabled,
            typingAnimationDurationMs = settings.TypingAnimationDurationMs,
            coordinatedTextCursorAnimationEnabled = settings.CoordinatedTextCursorAnimationEnabled,
            smoothCursorEnabled = settings.SmoothCursorEnabled,
            smoothCursorDurationMs = settings.SmoothCursorDurationMs
        });
        var jsonPtr = ToUtf8(json);
        try
        {
            var resultPtr = writer_core_save_local_settings(jsonPtr);
            var resultJson = PtrToStringAndFree(resultPtr);
            var env = ParseEnvelope(resultJson);
            ThrowIfFailed(env);
            return Task.CompletedTask;
        }
        finally
        {
            Marshal.FreeHGlobal(jsonPtr);
        }
    }

    public int CalculateWordCount(string text)
    {
        var textPtr = ToUtf8(text);
        try
        {
            return writer_core_calculate_word_count(textPtr);
        }
        finally
        {
            Marshal.FreeHGlobal(textPtr);
        }
    }

    public Task<IReadOnlyList<ThemePaletteRecordDto>> ListPaletteRecordsAsync()
    {
        var resultPtr = writer_core_list_palette_records();
        var json = PtrToStringAndFree(resultPtr);
        var env = ParseEnvelope(json);
        ThrowIfFailed(env);

        var list = new List<ThemePaletteRecordDto>();
        if (env.Data?.ValueKind == JsonValueKind.Array)
        {
            foreach (var item in env.Data.Value.EnumerateArray())
            {
                list.Add(ParsePaletteRecord(item));
            }
        }
        return Task.FromResult<IReadOnlyList<ThemePaletteRecordDto>>(list);
    }

    public Task<IReadOnlyList<BuiltinThemeDto>> ListBuiltinThemesAsync()
    {
        var resultPtr = writer_core_list_builtin_themes();
        var json = PtrToStringAndFree(resultPtr);
        var env = ParseEnvelope(json);
        ThrowIfFailed(env);

        var list = new List<BuiltinThemeDto>();
        if (env.Data?.ValueKind == JsonValueKind.Array)
        {
            foreach (var item in env.Data.Value.EnumerateArray())
            {
                list.Add(ParseBuiltinTheme(item));
            }
        }
        return Task.FromResult<IReadOnlyList<BuiltinThemeDto>>(list);
    }

    private static ThemeColorSchemeDto ParseColorScheme(JsonElement el)
    {
        var s = new ThemeColorSchemeDto();
        if (el.ValueKind != JsonValueKind.Object) return s;
        if (el.TryGetProperty("primary", out var p)) s.Primary = p.GetString() ?? "";
        if (el.TryGetProperty("on_primary", out var op)) s.OnPrimary = op.GetString() ?? "";
        if (el.TryGetProperty("primary_container", out var pc)) s.PrimaryContainer = pc.GetString() ?? "";
        if (el.TryGetProperty("on_primary_container", out var opc)) s.OnPrimaryContainer = opc.GetString() ?? "";
        if (el.TryGetProperty("inverse_primary", out var ip)) s.InversePrimary = ip.GetString() ?? "";
        if (el.TryGetProperty("secondary", out var sec)) s.Secondary = sec.GetString() ?? "";
        if (el.TryGetProperty("on_secondary", out var osec)) s.OnSecondary = osec.GetString() ?? "";
        if (el.TryGetProperty("secondary_container", out var sc)) s.SecondaryContainer = sc.GetString() ?? "";
        if (el.TryGetProperty("on_secondary_container", out var osc)) s.OnSecondaryContainer = osc.GetString() ?? "";
        if (el.TryGetProperty("tertiary", out var ter)) s.Tertiary = ter.GetString() ?? "";
        if (el.TryGetProperty("on_tertiary", out var oter)) s.OnTertiary = oter.GetString() ?? "";
        if (el.TryGetProperty("tertiary_container", out var tc)) s.TertiaryContainer = tc.GetString() ?? "";
        if (el.TryGetProperty("on_tertiary_container", out var otc)) s.OnTertiaryContainer = otc.GetString() ?? "";
        if (el.TryGetProperty("background", out var bg)) s.Background = bg.GetString() ?? "";
        if (el.TryGetProperty("on_background", out var obg)) s.OnBackground = obg.GetString() ?? "";
        if (el.TryGetProperty("surface", out var sf)) s.Surface = sf.GetString() ?? "";
        if (el.TryGetProperty("on_surface", out var osf)) s.OnSurface = osf.GetString() ?? "";
        if (el.TryGetProperty("surface_variant", out var sv)) s.SurfaceVariant = sv.GetString() ?? "";
        if (el.TryGetProperty("on_surface_variant", out var osv)) s.OnSurfaceVariant = osv.GetString() ?? "";
        if (el.TryGetProperty("surface_tint", out var st)) s.SurfaceTint = st.GetString() ?? "";
        if (el.TryGetProperty("surface_dim", out var sd)) s.SurfaceDim = sd.GetString() ?? "";
        if (el.TryGetProperty("surface_bright", out var sb)) s.SurfaceBright = sb.GetString() ?? "";
        if (el.TryGetProperty("surface_container_lowest", out var scl)) s.SurfaceContainerLowest = scl.GetString() ?? "";
        if (el.TryGetProperty("surface_container_low", out var sclw)) s.SurfaceContainerLow = sclw.GetString() ?? "";
        if (el.TryGetProperty("surface_container", out var sc2)) s.SurfaceContainer = sc2.GetString() ?? "";
        if (el.TryGetProperty("surface_container_high", out var sch)) s.SurfaceContainerHigh = sch.GetString() ?? "";
        if (el.TryGetProperty("surface_container_highest", out var schs)) s.SurfaceContainerHighest = schs.GetString() ?? "";
        if (el.TryGetProperty("inverse_surface", out var isf)) s.InverseSurface = isf.GetString() ?? "";
        if (el.TryGetProperty("inverse_on_surface", out var iosf)) s.InverseOnSurface = iosf.GetString() ?? "";
        if (el.TryGetProperty("error", out var err)) s.Error = err.GetString() ?? "";
        if (el.TryGetProperty("on_error", out var oerr)) s.OnError = oerr.GetString() ?? "";
        if (el.TryGetProperty("error_container", out var ec)) s.ErrorContainer = ec.GetString() ?? "";
        if (el.TryGetProperty("on_error_container", out var oec)) s.OnErrorContainer = oec.GetString() ?? "";
        if (el.TryGetProperty("outline", out var ol)) s.Outline = ol.GetString() ?? "";
        if (el.TryGetProperty("outline_variant", out var olv)) s.OutlineVariant = olv.GetString() ?? "";
        if (el.TryGetProperty("scrim", out var scr)) s.Scrim = scr.GetString() ?? "";
        return s;
    }

    private static ThemePaletteRecordDto ParsePaletteRecord(JsonElement el)
    {
        var r = new ThemePaletteRecordDto();
        if (el.ValueKind != JsonValueKind.Object) return r;
        if (el.TryGetProperty("schema_version", out var sv)) r.SchemaVersion = sv.GetUInt32();
        if (el.TryGetProperty("palette_id", out var pid)) r.PaletteId = pid.GetString() ?? "";
        if (el.TryGetProperty("palette_fingerprint", out var pf)) r.PaletteFingerprint = pf.GetString() ?? "";
        if (el.TryGetProperty("source", out var src)) r.Source = src.GetString() ?? "";
        if (el.TryGetProperty("source_platform", out var sp)) r.SourcePlatform = sp.GetString() ?? "";
        if (el.TryGetProperty("source_device_id", out var sdi)) r.SourceDeviceId = sdi.GetString() ?? "";
        if (el.TryGetProperty("source_device_class", out var sdc)) r.SourceDeviceClass = sdc.GetString() ?? "";
        if (el.TryGetProperty("captured_at_ms", out var cam)) r.CapturedAtMs = cam.GetInt64();
        if (el.TryGetProperty("variant", out var v)) r.Variant = v.GetString() ?? "";
        if (el.TryGetProperty("light_scheme", out var ls)) r.LightScheme = ParseColorScheme(ls);
        if (el.TryGetProperty("dark_scheme", out var ds)) r.DarkScheme = ParseColorScheme(ds);
        return r;
    }

    private static BuiltinThemeDto ParseBuiltinTheme(JsonElement el)
    {
        var t = new BuiltinThemeDto();
        if (el.ValueKind != JsonValueKind.Object) return t;
        if (el.TryGetProperty("theme_id", out var tid)) t.ThemeId = tid.GetString() ?? "";
        if (el.TryGetProperty("name", out var n)) t.Name = n.GetString() ?? "";
        if (el.TryGetProperty("light_scheme", out var ls)) t.LightScheme = ParseColorScheme(ls);
        if (el.TryGetProperty("dark_scheme", out var ds)) t.DarkScheme = ParseColorScheme(ds);
        return t;
    }

    public Task<SyncConfigDto> LoadSyncConfigAsync()
    {
        var resultPtr = writer_core_load_sync_config();
        var json = PtrToStringAndFree(resultPtr);
        var env = ParseEnvelope(json);
        ThrowIfFailed(env);

        var cfg = new SyncConfigDto("", "", false, 5);
        if (env.Data?.ValueKind == JsonValueKind.Object)
        {
            var remoteUrl = env.Data.Value.TryGetProperty("remoteUrl", out var ru) ? ru.GetString() ?? "" : "";
            var accessToken = env.Data.Value.TryGetProperty("accessToken", out var at) ? at.GetString() ?? "" : "";
            var autoSync = env.Data.Value.TryGetProperty("autoSync", out var asv) && asv.GetBoolean();
            var interval = env.Data.Value.TryGetProperty("intervalMinutes", out var im) ? im.GetInt32() : 5;
            cfg = new SyncConfigDto(remoteUrl, accessToken, autoSync, interval);
        }
        return Task.FromResult(cfg);
    }

    public Task SaveSyncConfigAsync(SyncConfigDto config)
    {
        var json = JsonSerializer.Serialize(new
        {
            remoteUrl = config.RemoteUrl,
            accessToken = config.AccessToken,
            autoSync = config.AutoSync,
            intervalMinutes = config.IntervalMinutes
        });
        var jsonPtr = ToUtf8(json);
        try
        {
            var resultPtr = writer_core_save_sync_config(jsonPtr);
            var resultJson = PtrToStringAndFree(resultPtr);
            var env = ParseEnvelope(resultJson);
            ThrowIfFailed(env);
            return Task.CompletedTask;
        }
        finally
        {
            Marshal.FreeHGlobal(jsonPtr);
        }
    }

    public Task<string> PerformSyncAsync()
    {
        var resultPtr = writer_core_perform_sync();
        var json = PtrToStringAndFree(resultPtr);
        var env = ParseEnvelope(json);
        ThrowIfFailed(env);
        return Task.FromResult(json ?? "");
    }

    public Task<string> SyncDryRunAsync()
    {
        var resultPtr = writer_core_sync_dry_run();
        var json = PtrToStringAndFree(resultPtr);
        var env = ParseEnvelope(json);
        ThrowIfFailed(env);
        return Task.FromResult(json ?? "");
    }

    public Task<WritingStatsDto> GetWritingStatsAsync()
    {
        var resultPtr = writer_core_get_writing_stats();
        var json = PtrToStringAndFree(resultPtr);
        var env = ParseEnvelope(json);
        ThrowIfFailed(env);

        var stats = new WritingStatsDto(0, 0, 0, 0, "");
        if (env.Data?.ValueKind == JsonValueKind.Object)
        {
            int totalWords = 0, todayWords = 0, sessionWords = 0, streakDays = 0;
            string sessionStart = "";
            if (env.Data.Value.TryGetProperty("totalWords", out var tw)) totalWords = tw.GetInt32();
            if (env.Data.Value.TryGetProperty("todayWords", out var tdw)) todayWords = tdw.GetInt32();
            if (env.Data.Value.TryGetProperty("sessionWords", out var sw)) sessionWords = sw.GetInt32();
            if (env.Data.Value.TryGetProperty("streakDays", out var sd)) streakDays = sd.GetInt32();
            if (env.Data.Value.TryGetProperty("sessionStartTime", out var ss)) sessionStart = ss.GetString() ?? "";
            stats = new WritingStatsDto(totalWords, todayWords, sessionWords, streakDays, sessionStart);
        }
        return Task.FromResult(stats);
    }

    public Task<IReadOnlyList<StarmapSummaryDto>> ListStarmapsAsync(string projectId)
    {
        var pidPtr = ToUtf8(projectId);
        try
        {
            var resultPtr = writer_core_list_starmaps_for_project(pidPtr);
            var json = PtrToStringAndFree(resultPtr);
            var env = ParseEnvelope(json);
            ThrowIfFailed(env);

            var list = new List<StarmapSummaryDto>();
            if (env.Data?.ValueKind == JsonValueKind.Array)
            {
                foreach (var sm in env.Data.Value.EnumerateArray())
                {
                    var id = sm.GetProperty("id").GetString() ?? "";
                    var name = sm.GetProperty("name").GetString() ?? "";
                    var nodeCount = sm.TryGetProperty("nodeCount", out var nc) ? nc.GetInt32() : 0;
                    var edgeCount = sm.TryGetProperty("edgeCount", out var ec) ? ec.GetInt32() : 0;
                    list.Add(new StarmapSummaryDto(id, name, nodeCount, edgeCount));
                }
            }
            return Task.FromResult<IReadOnlyList<StarmapSummaryDto>>(list);
        }
        finally
        {
            Marshal.FreeHGlobal(pidPtr);
        }
    }

    public Task<string> GetStarmapAsync(string starmapId)
    {
        var sidPtr = ToUtf8(starmapId);
        try
        {
            var resultPtr = writer_core_get_starmap(sidPtr);
            var json = PtrToStringAndFree(resultPtr);
            var env = ParseEnvelope(json);
            ThrowIfFailed(env);
            return Task.FromResult(json ?? "");
        }
        finally
        {
            Marshal.FreeHGlobal(sidPtr);
        }
    }

    public Task<StarmapSummaryDto> CreateStarmapAsync(string projectId, string name)
    {
        var pidPtr = ToUtf8(projectId);
        var namePtr = ToUtf8(name);
        try
        {
            var resultPtr = writer_core_create_starmap(pidPtr, namePtr);
            var json = PtrToStringAndFree(resultPtr);
            var env = ParseEnvelope(json);
            ThrowIfFailed(env);

            var id = env.Data?.GetProperty("id").GetString() ?? "";
            var smName = env.Data?.GetProperty("name").GetString() ?? name;
            return Task.FromResult(new StarmapSummaryDto(id, smName, 0, 0));
        }
        finally
        {
            Marshal.FreeHGlobal(pidPtr);
            Marshal.FreeHGlobal(namePtr);
        }
    }

    public string? GetEditorVisualTransaction(
        string oldText, string newText, uint oldCursorIndex, uint newCursorIndex,
        string cause, uint maxAnimatedChars, uint animationDurationMs)
    {
        var oldTextPtr = ToUtf8(oldText);
        var newTextPtr = ToUtf8(newText);
        var causePtr = ToUtf8(cause);
        try
        {
            var resultPtr = writer_core_editor_visual_transaction(
                oldTextPtr, newTextPtr, oldCursorIndex, newCursorIndex,
                causePtr, maxAnimatedChars, animationDurationMs);
            return PtrToStringAndFree(resultPtr);
        }
        finally
        {
            Marshal.FreeHGlobal(oldTextPtr);
            Marshal.FreeHGlobal(newTextPtr);
            Marshal.FreeHGlobal(causePtr);
        }
    }
}

public sealed class LocalSettings
{
    public float FontSize { get; set; } = 16f;
    public float LineHeight { get; set; } = 1.5f;
    public string Theme { get; set; } = "system";
    public string AppearanceMode { get; set; } = "system";
    public string ColorSource { get; set; } = "built_in";
    public bool DynamicColorEnabled { get; set; } = false;
    public string SelectedBuiltinThemeId { get; set; } = "";
    public string SelectedPaletteId { get; set; } = "";
    public bool AutoSave { get; set; } = true;
    public bool AutoIndent { get; set; } = true;
    public bool TypingAnimationEnabled { get; set; } = true;
    public int TypingAnimationDurationMs { get; set; } = 100;
    public bool CoordinatedTextCursorAnimationEnabled { get; set; } = true;
    public bool SmoothCursorEnabled { get; set; } = true;
    public int SmoothCursorDurationMs { get; set; } = 80;
}

public sealed class ThemeColorSchemeDto
{
    public string Primary { get; set; } = "";
    public string OnPrimary { get; set; } = "";
    public string PrimaryContainer { get; set; } = "";
    public string OnPrimaryContainer { get; set; } = "";
    public string InversePrimary { get; set; } = "";
    public string Secondary { get; set; } = "";
    public string OnSecondary { get; set; } = "";
    public string SecondaryContainer { get; set; } = "";
    public string OnSecondaryContainer { get; set; } = "";
    public string Tertiary { get; set; } = "";
    public string OnTertiary { get; set; } = "";
    public string TertiaryContainer { get; set; } = "";
    public string OnTertiaryContainer { get; set; } = "";
    public string Background { get; set; } = "";
    public string OnBackground { get; set; } = "";
    public string Surface { get; set; } = "";
    public string OnSurface { get; set; } = "";
    public string SurfaceVariant { get; set; } = "";
    public string OnSurfaceVariant { get; set; } = "";
    public string SurfaceTint { get; set; } = "";
    public string SurfaceDim { get; set; } = "";
    public string SurfaceBright { get; set; } = "";
    public string SurfaceContainerLowest { get; set; } = "";
    public string SurfaceContainerLow { get; set; } = "";
    public string SurfaceContainer { get; set; } = "";
    public string SurfaceContainerHigh { get; set; } = "";
    public string SurfaceContainerHighest { get; set; } = "";
    public string InverseSurface { get; set; } = "";
    public string InverseOnSurface { get; set; } = "";
    public string Error { get; set; } = "";
    public string OnError { get; set; } = "";
    public string ErrorContainer { get; set; } = "";
    public string OnErrorContainer { get; set; } = "";
    public string Outline { get; set; } = "";
    public string OutlineVariant { get; set; } = "";
    public string Scrim { get; set; } = "";
}

public sealed class ThemePaletteRecordDto
{
    public uint SchemaVersion { get; set; }
    public string PaletteId { get; set; } = "";
    public string PaletteFingerprint { get; set; } = "";
    public string Source { get; set; } = "";
    public string SourcePlatform { get; set; } = "";
    public string SourceDeviceId { get; set; } = "";
    public string SourceDeviceClass { get; set; } = "";
    public long CapturedAtMs { get; set; }
    public string Variant { get; set; } = "";
    public ThemeColorSchemeDto? LightScheme { get; set; }
    public ThemeColorSchemeDto? DarkScheme { get; set; }
}

public sealed class BuiltinThemeDto
{
    public string ThemeId { get; set; } = "";
    public string Name { get; set; } = "";
    public ThemeColorSchemeDto? LightScheme { get; set; }
    public ThemeColorSchemeDto? DarkScheme { get; set; }
}

public sealed class WriterCoreException : Exception
{
    public string Code { get; }
    public string? MessageKey { get; }
    public Dictionary<string, string>? MessageArgs { get; }

    public WriterCoreException(string code, string? messageKey, Dictionary<string, string>? messageArgs, string message)
        : base($"[{code}] {message}")
    {
        Code = code;
        MessageKey = messageKey;
        MessageArgs = messageArgs;
    }

    public WriterCoreException(string code, string message) : base($"[{code}] {message}")
    {
        Code = code;
    }
}
