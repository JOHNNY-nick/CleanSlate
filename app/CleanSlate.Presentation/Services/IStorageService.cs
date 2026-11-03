namespace CleanSlate.Presentation.Services;

/// <summary>Disks of the PC and journal of the free space around each action (D69).</summary>
public interface IStorageService
{
    /// <summary>Internal disks with their total and free space.</summary>
    IReadOnlyList<DriveSpace> Drives();

    /// <summary>Recorded actions, oldest first.</summary>
    IReadOnlyList<StorageEntry> Journal();

    /// <summary>
    /// Runs an action of the engine between two measures of the system disk and of the disk of
    /// the quarantine, and records it (nothing is recorded if it fails).
    /// </summary>
    Task<T> MeasureAsync<T>(StorageOperation operation, Func<T, string> describe, Func<Task<T>> action, Func<T, ulong> reported, Func<T, Guid?>? sessionOf = null);

    /// <summary>A quarantine moved these items: the cached maps follow, without a new walk (D73).</summary>
    void RecordCleaned(CleanSlate.Interop.Session session);

    /// <summary>A purge or an immediate deletion freed these bytes of the quarantine.</summary>
    void RecordPurged(ulong bytes);

    /// <summary>Last storage map computed for a drive (« C: »), or null (D73).</summary>
    CachedDiskMap? CachedMap(string drive);

    /// <summary>Keeps a freshly computed map, with the time of the computation.</summary>
    void SaveMap(string drive, CleanSlate.Interop.DiskMap map);
}

/// <summary>A storage map and when it was computed (D73).</summary>
/// <param name="Map">The map, kept up to date after each cleanup.</param>
/// <param name="ComputedAt">When the disk was walked.</param>
public sealed record CachedDiskMap(CleanSlate.Interop.DiskMap Map, DateTimeOffset ComputedAt);
