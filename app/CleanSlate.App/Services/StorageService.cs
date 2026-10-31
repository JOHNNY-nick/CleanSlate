using System.Text.Json;
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
    public Task<T> MeasureAsync<T>(StorageOperation operation, Func<T, string> describe, Func<Task<T>> action, Func<T, ulong> reported)
    {
        string quarantine = string.IsNullOrWhiteSpace(_settings.QuarantineFolder) ? _folders.DefaultQuarantineFolder : _settings.QuarantineFolder;
        var drives = new[] { Path.GetPathRoot(Environment.SystemDirectory), Path.GetPathRoot(quarantine) }
            .Where(d => !string.IsNullOrEmpty(d))
            .Select(d => d!.TrimEnd('\\'));
        return StorageMeasure.RunAsync(operation, describe, drives, FreeSpace, () => DateTimeOffset.Now, Record, action, reported);
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
