using System.Runtime.InteropServices;
using System.Text.Json;

namespace Sujian.Windows.Bridge;

public sealed record WorkspaceSummary(IReadOnlyList<ProjectSummary> Projects);
public sealed record ProjectSummary(string Id, string Name);
public sealed record VolumeSummary(string Id, string Name);
public sealed record ChapterSummary(string Id, string Title);

internal sealed class EnvelopeResult
{
    public bool Success { get; set; }
    public JsonElement? Data { get; set; }
    public string? ErrorCode { get; set; }
    public string? UserMessage { get; set; }
}

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
    private static extern int writer_core_calculate_word_count(IntPtr text);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern void writer_core_free_string(IntPtr ptr);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr writer_core_get_last_error();

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
        return JsonSerializer.Deserialize<EnvelopeResult>(json, new JsonSerializerOptions { PropertyNameCaseInsensitive = true })
            ?? new EnvelopeResult { Success = false, ErrorCode = "PARSE_ERROR" };
    }

    private static void ThrowIfFailed(EnvelopeResult env)
    {
        if (!env.Success)
            throw new WriterCoreException(env.ErrorCode ?? "UNKNOWN", env.UserMessage ?? "Unknown error");
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
            if (env.Data.Value.TryGetProperty("autoSave", out var asv)) s.AutoSave = asv.GetBoolean();
            if (env.Data.Value.TryGetProperty("autoIndent", out var ai)) s.AutoIndent = ai.GetBoolean();
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
            autoSave = settings.AutoSave,
            autoIndent = settings.AutoIndent
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
}

public sealed class LocalSettings
{
    public float FontSize { get; set; } = 16f;
    public float LineHeight { get; set; } = 1.5f;
    public string Theme { get; set; } = "system";
    public bool AutoSave { get; set; } = true;
    public bool AutoIndent { get; set; } = true;
}

public sealed class WriterCoreException : Exception
{
    public string Code { get; }
    public WriterCoreException(string code, string message) : base($"[{code}] {message}")
    {
        Code = code;
    }
}
