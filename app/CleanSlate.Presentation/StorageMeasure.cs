namespace CleanSlate.Presentation;

/// <summary>Action of CleanSlate that can change the free space of a disk (D69).</summary>
public enum StorageOperation
{
    /// <summary>Items moved to the quarantine (same disk: no space freed yet).</summary>
    Quarantine,

    /// <summary>Old sessions of the quarantine deleted for good.</summary>
    Purge,

    /// <summary>A session deleted at once, without quarantine (D66).</summary>
    DeleteForGood,

    /// <summary>An application uninstalled by its own uninstaller.</summary>
    Uninstall,
}

/// <summary>Free space of one disk around one action (D69).</summary>
/// <param name="When">End of the action.</param>
/// <param name="Operation">What CleanSlate did.</param>
/// <param name="Drive">Disk, e.g. « C: ».</param>
/// <param name="FreeBefore">Free bytes before the action.</param>
/// <param name="FreeAfter">Free bytes after the action.</param>
/// <param name="ReportedBytes">Size handled, as counted by the engine.</param>
/// <param name="Detail">Short description: application, number of items, delay.</param>
/// <param name="SessionId">Quarantine session concerned, to show the detail in Historique (D73).</param>
public sealed record StorageEntry(
    DateTimeOffset When,
    StorageOperation Operation,
    string Drive,
    ulong FreeBefore,
    ulong FreeAfter,
    ulong ReportedBytes,
    string Detail,
    Guid? SessionId = null);

/// <summary>A disk of the PC: total and free space.</summary>
/// <param name="Name">« C: ».</param>
/// <param name="TotalBytes">Size of the disk.</param>
/// <param name="FreeBytes">Free space now.</param>
public sealed record DriveSpace(string Name, ulong TotalBytes, ulong FreeBytes);

/// <summary>
/// Measures the free space of the disks concerned before and after an action and records one
/// <see cref="StorageEntry"/> per disk (D69). A failed action records nothing. The measure is
/// the disk's own: another program writing at the same time changes it too.
/// </summary>
public static class StorageMeasure
{
    /// <summary>Entries kept in the journal.</summary>
    public const int MaxEntries = 300;

    /// <summary>Runs <paramref name="action"/> between two measures of each drive.</summary>
    public static async Task<T> RunAsync<T>(
        StorageOperation operation,
        Func<T, string> describe,
        IEnumerable<string> drives,
        Func<string, ulong> freeSpace,
        Func<DateTimeOffset> clock,
        Action<StorageEntry> record,
        Func<Task<T>> action,
        Func<T, ulong> reported,
        Func<T, Guid?>? sessionOf = null)
    {
        var before = drives
            .Select(d => d.ToUpperInvariant())
            .Distinct()
            .ToDictionary(d => d, freeSpace);
        T result = await action();
        if (operation == StorageOperation.Purge && reported(result) == 0)
        {
            return result; // the automatic purge of each start, with nothing old enough
        }

        DateTimeOffset when = clock();
        string detail = describe(result);
        foreach ((string drive, ulong free) in before)
        {
            record(new StorageEntry(when, operation, drive, free, freeSpace(drive), reported(result), detail, sessionOf?.Invoke(result)));
        }

        return result;
    }

    /// <summary>« +2 Go », « −300 Mo », « 0 octets ».</summary>
    public static string Difference(ulong before, ulong after) => after == before
        ? Formats.Size(0)
        : after > before ? $"+{Formats.Size(after - before)}" : $"−{Formats.Size(before - after)}";

    /// <summary>Adds an entry and keeps the <paramref name="max"/> most recent ones.</summary>
    public static IReadOnlyList<StorageEntry> Append(IEnumerable<StorageEntry> journal, StorageEntry entry, int max = MaxEntries) =>
        journal.Append(entry).OrderBy(e => e.When).TakeLast(max).ToList();
}
