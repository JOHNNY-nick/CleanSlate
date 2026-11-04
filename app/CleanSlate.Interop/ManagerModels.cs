using System.Text.Json.Serialization;

namespace CleanSlate.Interop;

/// <summary>Level of precaution of a location (D79): sensitive zones need the expert mode.</summary>
[JsonConverter(typeof(SnakeCaseEnumConverter<Zone>))]
public enum Zone
{
    /// <summary>The usual confirmation.</summary>
    Normal,

    /// <summary>Never proposed automatically; by hand in expert mode with « CONFIRMER ».</summary>
    Sensitive,
}

/// <summary>How a deletion is done (EF-23).</summary>
[JsonConverter(typeof(SnakeCaseEnumConverter<DeleteMode>))]
public enum DeleteMode
{
    /// <summary>« Sûr »: Recycle Bin, restorable.</summary>
    Safe,

    /// <summary>« Immédiat »: for good, the space is free at once.</summary>
    Immediate,
}

/// <summary>What a file is (« Catégories »).</summary>
[JsonConverter(typeof(SnakeCaseEnumConverter<FileCategory>))]
public enum FileCategory
{
    /// <summary>Videos.</summary>
    Videos,

    /// <summary>Photos.</summary>
    Photos,

    /// <summary>Music.</summary>
    Music,

    /// <summary>Documents.</summary>
    Documents,

    /// <summary>Games.</summary>
    Games,

    /// <summary>Developer tools.</summary>
    DevTools,

    /// <summary>Virtual machines.</summary>
    VirtualMachines,

    /// <summary>Phone backups.</summary>
    PhoneBackups,

    /// <summary>Installers downloaded.</summary>
    Installers,

    /// <summary>Archives.</summary>
    Archives,

    /// <summary>Programs.</summary>
    Applications,

    /// <summary>Windows and system.</summary>
    System,

    /// <summary>Caches.</summary>
    Caches,

    /// <summary>Anything else.</summary>
    Other,
}

/// <summary>A clean-up of the « Système » tab (EF-26, EF-31).</summary>
[JsonConverter(typeof(SnakeCaseEnumConverter<SystemTask>))]
public enum SystemTask
{
    /// <summary>Component store (DISM).</summary>
    ComponentStore,

    /// <summary>Windows Update download cache.</summary>
    UpdateCache,

    /// <summary>Delivery Optimization cache.</summary>
    DeliveryOptimization,

    /// <summary>Previous installation of Windows.</summary>
    WindowsOld,

    /// <summary>Hibernation file.</summary>
    Hibernation,

    /// <summary>Old restore points.</summary>
    RestorePoints,
}

/// <summary>A file found by an analysis.</summary>
public sealed record FileEntry(string Path, ulong SizeBytes, DateTimeOffset? Modified, FileCategory Category, Zone Zone);

/// <summary>Size of a category and its largest files.</summary>
public sealed record CategoryTotal(FileCategory Category, ulong SizeBytes, ulong Files, IReadOnlyList<FileEntry> Largest);

/// <summary>Result of the analysis of a drive (read-only).</summary>
public sealed record StorageAnalysis(
    string Root,
    IReadOnlyList<FileEntry> Largest,
    IReadOnlyList<CategoryTotal> Categories,
    IReadOnlyList<FileEntry> OldFiles,
    IReadOnlyList<string> EmptyFolders,
    IReadOnlyList<IReadOnlyList<string>> DuplicateCandidates,
    ulong Files,
    ulong SizeBytes,
    ulong Denied,
    bool Cancelled);

/// <summary>Progress of a walk of the Gestionnaire.</summary>
public sealed record ManagerProgress(ulong Files, ulong Bytes, string Current);

/// <summary>Identical files: the copy to keep first.</summary>
public sealed record DuplicateGroup(ulong SizeBytes, string Sha256, string Keep, IReadOnlyList<FileEntry> Files);

/// <summary>A regenerable cache (EF-25).</summary>
public sealed record CacheItem(string Name, string Path, ulong SizeBytes, ulong Files, string? Browser, Zone Zone);

/// <summary>Recycle Bin of a drive.</summary>
public sealed record RecycleBinInfo(string Drive, ulong SizeBytes, ulong Items);

/// <summary>A drive or partition (display only).</summary>
public sealed record VolumeInfo(string Root, string Label, string FileSystem, string Kind, ulong TotalBytes, ulong FreeBytes);

/// <summary>A child of a folder, in the explorer under the storage map (D83).</summary>
/// <param name="Name">Name.</param>
/// <param name="Path">Full path.</param>
/// <param name="IsFolder">A folder.</param>
/// <param name="SizeBytes">Bytes on the disk; null for a folder whose size the map has.</param>
/// <param name="Modified">Last modification.</param>
/// <param name="Category">Category of a file.</param>
/// <param name="Zone">Sensitive zone or not.</param>
public sealed record FolderEntry(string Name, string Path, bool IsFolder, ulong? SizeBytes, DateTimeOffset? Modified, FileCategory Category, Zone Zone);

/// <summary>A physical disk and its unallocated space (display only).</summary>
public sealed record DiskInfo(uint Number, string Name, ulong SizeBytes, ulong UnallocatedBytes);

/// <summary>Expert mode and word typed, for the sensitive zones.</summary>
public sealed record ManualAction(bool ExpertMode = false, string? Confirmation = null);

/// <summary>What happened to one path.</summary>
public sealed record ActionOutcome(string Path, ulong SizeBytes, Zone? Zone, string Done, ItemError? Error);

/// <summary>Result of an action of the Gestionnaire.</summary>
public sealed record ActionResult(IReadOnlyList<ActionOutcome> Items, ulong FreedNowBytes, ulong FreedLaterBytes, bool Simulated);

/// <summary>A line of the « Système » tab.</summary>
public sealed record SystemItem(SystemTask Task, ulong? SizeBytes, bool Available);

/// <summary>What a clean-up of Windows did.</summary>
public sealed record SystemOutcome(ulong FreedBytes, bool RestartNeeded, bool Simulated, IReadOnlyList<string> Steps);

/// <summary>A folder and its size.</summary>
public sealed record FolderSize(string Path, ulong Bytes);

/// <summary>A measure of a drive (« Évolution »).</summary>
public sealed record DriveMeasure(DateTimeOffset TakenAt, string Drive, ulong TotalBytes, ulong FreeBytes, IReadOnlyList<FolderSize> Largest);
