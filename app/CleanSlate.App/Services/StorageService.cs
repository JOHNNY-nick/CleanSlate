using System.Text.Json;
using CleanSlate.Interop;
using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;

namespace CleanSlate.App.Services;

/// <summary>
/// <see cref="IStorageService"/> on this PC (D69): internal disks through <see cref="DriveInfo"/>,
/// journal in <c>%LOCALAPPDATA%\CleanSlate\storage.json</c> (the 300 most recent actions).
/// </summary>
public sealed class StorageService : IStorageService
{
    private static readonly string FilePath = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "CleanSlate", "storage.json");

    private readonly ISettingsService _settings;
    private readonly IFolderService _folders;
    private readonly object _gate = new();

    /// <summary>Creates the service.</summary>
    public StorageService(ISettingsService settings, IFolderService folders)
    {
        _settings = settings;
        _folders = folders;
    }

    /// <inheritdoc/>
    public IReadOnlyList<DriveSpace> Drives() => DriveInfo.GetDrives()
        .Where(d => d.DriveType == DriveType.Fixed && d.IsReady)
        .Select(d => new DriveSpace(d.Name.TrimEnd('\\'), (ulong)d.TotalSize, (ulong)d.AvailableFreeSpace))
        .ToList();

    /// <inheritdoc/>
    public IReadOnlyList<StorageEntry> Journal()
    {
        lock (_gate)
        {
            return Load();
        }
    }

    /// <inheritdoc/>
    public CachedDiskMap? CachedMap(string drive)
    {
        lock (_gate)
        {
            try
            {
                string file = MapFile(drive);
                return File.Exists(file) ? JsonSerializer.Deserialize<CachedDiskMap>(File.ReadAllText(file)) : null;
            }
            catch (Exception ex) when (ex is IOException or JsonException or UnauthorizedAccessException or NotSupportedException)
            {
                return null;
            }
        }
    }

    /// <inheritdoc/>
    public void SaveMap(string drive, DiskMap map) => Write(drive, new CachedDiskMap(map, DateTimeOffset.Now));

    /// <inheritdoc/>
    public void RecordCleaned(Session session)
    {
        var moved = session.Items
            .Where(i => i.Status is QuarantineStatus.Quarantined or QuarantineStatus.PendingReboot)
            .Select(i => (i.OriginalLocation, i.SizeBytes))
            .ToList();
        UpdateMaps(map => DiskMapUpdate.Moved(map, moved, QuarantineFolder()));
    }

    /// <inheritdoc/>
    public void RecordPurged(ulong bytes) => UpdateMaps(map => DiskMapUpdate.Purged(map, bytes, QuarantineFolder()));

    // Every cached map follows a cleanup; the time of the walk stays the same.
    private void UpdateMaps(Func<DiskMap, DiskMap> update)
    {
        foreach (DriveSpace drive in Drives())
        {
            if (CachedMap(drive.Name) is { } cached)
            {
                Write(drive.Name, cached with { Map = update(cached.Map) });
            }
        }
    }

    private void Write(string drive, CachedDiskMap cached)
    {
        lock (_gate)
        {
            try
            {
                Directory.CreateDirectory(Path.GetDirectoryName(FilePath)!);
                File.WriteAllText(MapFile(drive), JsonSerializer.Serialize(cached));
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                // The next « Actualiser » computes it again.
            }
        }
    }

    private static string MapFile(string drive) =>
        Path.Combine(Path.GetDirectoryName(FilePath)!, $"diskmap-{drive.TrimEnd(':', '\\')}.json");

    private string QuarantineFolder() =>
        string.IsNullOrWhiteSpace(_settings.QuarantineFolder) ? _folders.DefaultQuarantineFolder : _settings.QuarantineFolder;

    /// <inheritdoc/>
    public Task<T> MeasureAsync<T>(StorageOperation operation, Func<T, string> describe, Func<Task<T>> action, Func<T, ulong> reported, Func<T, Guid?>? sessionOf = null)
    {
        string quarantine = QuarantineFolder();
        var drives = new[] { Path.GetPathRoot(Environment.SystemDirectory), Path.GetPathRoot(quarantine) }
            .Where(d => !string.IsNullOrEmpty(d))
            .Select(d => d!.TrimEnd('\\'));
        return StorageMeasure.RunAsync(operation, describe, drives, FreeSpace, () => DateTimeOffset.Now, Record, action, reported, sessionOf);
    }

    private static ulong FreeSpace(string drive)
    {
        try
        {
            return (ulong)new DriveInfo(drive).AvailableFreeSpace;
        }
        catch (Exception ex) when (ex is IOException or ArgumentException or UnauthorizedAccessException)
        {
            return 0;
        }
    }

    // A journal that cannot be written never blocks the action it describes.
    private void Record(StorageEntry entry)
    {
        lock (_gate)
        {
            try
            {
                IReadOnlyList<StorageEntry> journal = StorageMeasure.Append(Load(), entry);
                Directory.CreateDirectory(Path.GetDirectoryName(FilePath)!);
                File.WriteAllText(FilePath, JsonSerializer.Serialize(journal));
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                // The page will simply miss this action.
            }
        }
    }

    private static List<StorageEntry> Load()
    {
        try
        {
            return File.Exists(FilePath)
                ? JsonSerializer.Deserialize<List<StorageEntry>>(File.ReadAllText(FilePath)) ?? []
                : [];
        }
        catch (Exception ex) when (ex is IOException or JsonException or UnauthorizedAccessException)
        {
            return [];
        }
    }
}
