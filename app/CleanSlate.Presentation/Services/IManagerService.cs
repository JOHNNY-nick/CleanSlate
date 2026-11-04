using CleanSlate.Interop;

namespace CleanSlate.Presentation.Services;

/// <summary>
/// The Gestionnaire of the engine (D79). Analyses are read-only; actions are real (the
/// application has no simulation mode, D66) and are only called after the confirmation of
/// <see cref="IDialogService.ConfirmManagerAsync"/>.
/// </summary>
public interface IManagerService
{
    /// <summary>Largest files, categories, old files, empty folders, candidates for duplicates.</summary>
    Task<StorageAnalysis> AnalyzeAsync(string root, IProgress<ManagerProgress>? progress, CancellationToken cancellationToken);

    /// <summary>Files of <paramref name="folder"/> and below, largest first, at most <paramref name="max"/> (D82).</summary>
    Task<StorageAnalysis> ListFilesAsync(string folder, int max, IProgress<ManagerProgress>? progress, CancellationToken cancellationToken);

    /// <summary>Duplicates among the candidates of an analysis.</summary>
    Task<IReadOnlyList<DuplicateGroup>> DuplicatesAsync(IReadOnlyList<IReadOnlyList<string>> candidates, CancellationToken cancellationToken);

    /// <summary>Regenerable caches.</summary>
    Task<IReadOnlyList<CacheItem>> CachesAsync(CancellationToken cancellationToken);

    /// <summary>Recycle Bins by drive.</summary>
    Task<IReadOnlyList<RecycleBinInfo>> RecycleBinsAsync(CancellationToken cancellationToken);

    /// <summary>Drives and partitions.</summary>
    Task<IReadOnlyList<VolumeInfo>> VolumesAsync(CancellationToken cancellationToken);

    /// <summary>Physical disks and their unallocated space.</summary>
    Task<IReadOnlyList<DiskInfo>> DisksAsync(CancellationToken cancellationToken);

    /// <summary>Deletions waiting for the next restart.</summary>
    Task<IReadOnlyList<string>> PendingAtRestartAsync(CancellationToken cancellationToken);

    /// <summary>Items of the « Système » tab.</summary>
    Task<IReadOnlyList<SystemItem>> SystemItemsAsync(CancellationToken cancellationToken);

    /// <summary>Deletion in mode « Sûr » or « Immédiat ».</summary>
    Task<ActionResult> DeleteAsync(IReadOnlyList<string> paths, DeleteMode mode, ManualAction manual, CancellationToken cancellationToken);

    /// <summary>Deletion at the next restart.</summary>
    Task<ActionResult> DeleteAtRestartAsync(IReadOnlyList<string> paths, ManualAction manual, CancellationToken cancellationToken);

    /// <summary>Secure deletion.</summary>
    Task<ActionResult> SecureDeleteAsync(IReadOnlyList<string> paths, ManualAction manual, CancellationToken cancellationToken);

    /// <summary>Move to another drive, checked.</summary>
    Task<ActionResult> MoveAsync(IReadOnlyList<string> paths, string destination, ManualAction manual, CancellationToken cancellationToken);

    /// <summary>Emptying of caches.</summary>
    Task<ActionResult> CleanCachesAsync(IReadOnlyList<string> paths, ManualAction manual, CancellationToken cancellationToken);

    /// <summary>Emptying of the Recycle Bin of a drive, or of all.</summary>
    Task<ActionResult> EmptyRecycleBinAsync(string? drive, CancellationToken cancellationToken);

    /// <summary>A clean-up of Windows.</summary>
    Task<SystemOutcome> RunSystemAsync(SystemTask task, bool expertMode, CancellationToken cancellationToken);

    /// <summary>Cancels a deletion at the next restart.</summary>
    Task CancelAtRestartAsync(string path, CancellationToken cancellationToken);

    /// <summary>Records a measure of a drive.</summary>
    Task RecordMeasureAsync(DriveMeasure measure, CancellationToken cancellationToken);

    /// <summary>Measures of a drive, oldest first.</summary>
    Task<IReadOnlyList<DriveMeasure>> MeasuresAsync(string drive, CancellationToken cancellationToken);

    /// <summary>Writes a report on the Desktop (export folder) and returns its path.</summary>
    string SaveReport(string fileName, string content);

    /// <summary>Shows the item in the Explorer (EF-35).</summary>
    void Reveal(string path);

    /// <summary>Copies the path to the clipboard (EF-35).</summary>
    void CopyPath(string path);

    /// <summary>Opens the Windows properties of the item (EF-35).</summary>
    void ShowProperties(string path);
}
