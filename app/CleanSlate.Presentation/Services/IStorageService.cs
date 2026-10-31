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
    Task<T> MeasureAsync<T>(StorageOperation operation, Func<T, string> describe, Func<Task<T>> action, Func<T, ulong> reported);
}
