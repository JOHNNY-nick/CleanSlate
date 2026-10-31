using System.Collections.ObjectModel;
using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace CleanSlate.Presentation.ViewModels;

/// <summary>A disk on the « Stockage » page.</summary>
/// <param name="Name">« C: ».</param>
/// <param name="FreeText">Free space, « 120 Go ».</param>
/// <param name="TotalText">Size, « 500 Go ».</param>
/// <param name="UsedPercent">Used part, 0 to 100.</param>
public sealed record DriveRow(string Name, string FreeText, string TotalText, int UsedPercent);

/// <summary>An action on the « Stockage » page.</summary>
/// <param name="DateText">« 31 oct. 2026 11:05 ».</param>
/// <param name="Operation">What CleanSlate did (text from the resources).</param>
/// <param name="Drive">Disk.</param>
/// <param name="Detail">Application, number of items, delay.</param>
/// <param name="BeforeText">Free space before.</param>
/// <param name="AfterText">Free space after.</param>
/// <param name="DifferenceText">« +2 Go », « −300 Mo » or « 0 octets ».</param>
public sealed record StorageEntryRow(
    string DateText,
    StorageOperation Operation,
    string Drive,
    string Detail,
    string BeforeText,
    string AfterText,
    string DifferenceText);

/// <summary>
/// « Stockage » (D69): disks, space held by the quarantine, space freed for good, and the free
/// space before and after each action of CleanSlate.
/// </summary>
public sealed partial class StorageViewModel : ObservableObject
{
    private readonly IStorageService _storage;
    private readonly IEngineService _engine;

    /// <summary>Creates the ViewModel.</summary>
    public StorageViewModel(IStorageService storage, IEngineService engine)
    {
        _storage = storage;
        _engine = engine;
    }

    /// <summary>Internal disks.</summary>
    public ObservableCollection<DriveRow> Drives { get; } = [];

    /// <summary>Actions, most recent first.</summary>
    public ObservableCollection<StorageEntryRow> Entries { get; } = [];

    /// <summary>Size of the items still in quarantine (restorable, not freed yet).</summary>
    [ObservableProperty]
    private string _quarantineHeldText = Formats.Size(0);

    /// <summary>Space freed for good: purges and immediate deletions.</summary>
    [ObservableProperty]
    private string _freedForGoodText = Formats.Size(0);

    /// <summary>No action recorded yet.</summary>
    [ObservableProperty]
    private bool _isEmpty = true;

    /// <summary>Reads the disks, the quarantine and the journal.</summary>
    [RelayCommand]
    private async Task LoadAsync(CancellationToken cancellationToken)
    {
        Drives.Clear();
        foreach (DriveSpace d in _storage.Drives())
        {
            int used = d.TotalBytes == 0 ? 0 : (int)Math.Round(100.0 * (d.TotalBytes - d.FreeBytes) / d.TotalBytes);
            Drives.Add(new DriveRow(d.Name, Formats.Size(d.FreeBytes), Formats.Size(d.TotalBytes), used));
        }

        try
        {
            IReadOnlyList<Session> sessions = await _engine.ListSessionsAsync(cancellationToken);
            ulong held = sessions
                .Where(s => !s.Simulated)
                .SelectMany(s => s.Items)
                .Where(i => i.Status is QuarantineStatus.Quarantined or QuarantineStatus.PendingReboot)
                .Aggregate(0UL, (sum, i) => sum + i.SizeBytes);
            QuarantineHeldText = Formats.Size(held);
        }
        catch (EngineException)
        {
            QuarantineHeldText = Formats.Unknown;
        }

        IReadOnlyList<StorageEntry> journal = _storage.Journal();
        FreedForGoodText = Formats.Size(journal
            .Where(e => e.Operation is StorageOperation.Purge or StorageOperation.DeleteForGood)
            .Aggregate(0UL, (sum, e) => sum + e.ReportedBytes));
        Entries.Clear();
        foreach (StorageEntry e in journal.OrderByDescending(e => e.When))
        {
            Entries.Add(new StorageEntryRow(
                e.When.LocalDateTime.ToString("d MMM yyyy HH:mm", System.Globalization.CultureInfo.GetCultureInfo("fr-FR")),
                e.Operation,
                e.Drive,
                e.Detail,
                Formats.Size(e.FreeBefore),
                Formats.Size(e.FreeAfter),
                Difference(e.FreeBefore, e.FreeAfter)));
        }

        IsEmpty = Entries.Count == 0;
    }

    /// <summary>« +2 Go », « −300 Mo », « 0 octets ».</summary>
    public static string Difference(ulong before, ulong after) => after == before
        ? Formats.Size(0)
        : after > before ? $"+{Formats.Size(after - before)}" : $"−{Formats.Size(before - after)}";
}
