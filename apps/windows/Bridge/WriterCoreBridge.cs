namespace Sujian.Windows.Bridge;

public sealed record WorkspaceSummary(IReadOnlyList<ProjectSummary> Projects);
public sealed record ProjectSummary(string Id, string Name);
public sealed record VolumeSummary(string Id, string Name);
public sealed record ChapterSummary(string Id, string Title);

/// <summary>
/// Thin Windows UI bridge to Rust writer_core. Keep workspace format, chapter
/// persistence, sync and settings logic in core/writer_core; this layer only
/// adapts UI calls to the native binding surface.
/// </summary>
public sealed class WriterCoreBridge
{
    public Task<WorkspaceSummary> OpenWorkspaceAsync(string? path)
    {
        // TODO(issue #433): bind to writer_core once the Windows UniFFI/C ABI artifact is produced.
        return Task.FromResult(new WorkspaceSummary(Array.Empty<ProjectSummary>()));
    }

    public Task<IReadOnlyList<ProjectSummary>> ListProjectsAsync()
    {
        return Task.FromResult<IReadOnlyList<ProjectSummary>>(Array.Empty<ProjectSummary>());
    }

    public Task<IReadOnlyList<VolumeSummary>> ListVolumesAsync(string projectId)
    {
        return Task.FromResult<IReadOnlyList<VolumeSummary>>(Array.Empty<VolumeSummary>());
    }

    public Task<IReadOnlyList<ChapterSummary>> ListChaptersAsync(string projectId, string volumeId)
    {
        return Task.FromResult<IReadOnlyList<ChapterSummary>>(Array.Empty<ChapterSummary>());
    }

    public Task<string> OpenChapterAsync(string projectId, string volumeId, string chapterId)
    {
        return Task.FromResult(string.Empty);
    }

    public Task SaveChapterAsync(string chapterId, string plainText)
    {
        return Task.CompletedTask;
    }
}
