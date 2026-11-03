using System.Collections.ObjectModel;
using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace CleanSlate.Presentation.ViewModels;

/// <summary>One item of a past session, selectable for a partial restore.</summary>
public sealed partial class SessionItemViewModel : ObservableObject
{
    /// <summary>Wraps a quarantined item.</summary>
    public SessionItemViewModel(QuarantinedItem item)
    {
        Item = item;
        SizeText = Formats.Size(item.SizeBytes);
    }

    /// <summary>Engine record.</summary>
    public QuarantinedItem Item { get; }

    /// <summary>Original path or key.</summary>
    public string Location => Item.OriginalLocation;

    /// <summary>Kind, turned into text by the view.</summary>
    public LeftoverKind Kind => Item.Kind;

    /// <summary>State, turned into text by the view.</summary>
    public QuarantineStatus Status => Item.Status;

    /// <summary>Size text.</summary>
    public string SizeText { get; }

    /// <summary>Still in quarantine (or waiting for a reboot): restorable.</summary>
    public bool IsRestorable => Item.Status is QuarantineStatus.Quarantined or QuarantineStatus.PendingReboot;

    /// <summary>Selected for a partial restore.</summary>
    [ObservableProperty]
    private bool _isChecked;
}

/// <summary>Free space of one disk around one action of a session (D73).</summary>
/// <param name="Operation">What CleanSlate did (text from the resources).</param>
/// <param name="Drive">« C: ».</param>
/// <param name="BeforeText">Free space before.</param>
/// <param name="AfterText">Free space after.</param>
/// <param name="DifferenceText">« +2 Go », « −300 Mo » or « 0 octets ».</param>
public sealed record SessionStorageRow(StorageOperation Operation, string Drive, string BeforeText, string AfterText, string DifferenceText);

/// <summary>One past session (EF-20).</summary>
public sealed class SessionViewModel
{
    /// <summary>Wraps a session.</summary>
    public SessionViewModel(Session session, int quarantineDays, IEnumerable<StorageEntry>? journal = null)
    {
        StorageRows = (journal ?? [])
            .Where(e => e.SessionId == session.Id)
            .OrderBy(e => e.When)
            .Select(e => new SessionStorageRow(e.Operation, e.Drive, Formats.Size(e.FreeBefore), Formats.Size(e.FreeAfter), StorageMeasure.Difference(e.FreeBefore, e.FreeAfter)))
            .ToList();
        Session = session;
        DateText = Formats.Date(DateOnly.FromDateTime(session.CreatedAt.LocalDateTime));
        PurgeDateText = Formats.Date(DateOnly.FromDateTime(session.CreatedAt.LocalDateTime.AddDays(quarantineDays)));
        FreedText = Formats.Size(session.FreedBytes);
        Items = new ObservableCollection<SessionItemViewModel>(session.Items.Select(i => new SessionItemViewModel(i)));
    }

    /// <summary>Engine record.</summary>
    public Session Session { get; }

    /// <summary>Date, e.g. « 22 oct. 2026 ».</summary>
    public string DateText { get; }

    /// <summary>Date of the automatic purge.</summary>
    public string PurgeDateText { get; }

    /// <summary>
    /// What happens to the session: « purge le 30 nov. 2026 » while it waits in quarantine;
    /// once purged (the delay passed, « Purger », or the immediate deletion of D66) no future
    /// date is shown any more, and it says that nothing can be restored.
    /// </summary>
    public string PurgeNote => Session.Status switch
    {
        SessionStatus.Purged => "supprimée définitivement, plus restaurable",
        SessionStatus.Restored => "restaurée",
        _ => $"purge le {PurgeDateText}",
    };

    /// <summary>Space freed.</summary>
    public string FreedText { get; }

    /// <summary>Number of items.</summary>
    public int ItemCount => Items.Count;

    /// <summary>State, turned into text by the view.</summary>
    public SessionStatus Status => Session.Status;

    /// <summary>Started from the uninstall wizard (true) or the orphans scan.</summary>
    public bool IsFromApp => Session.Origin.Kind == "app";

    /// <summary>Items of the session.</summary>
    public ObservableCollection<SessionItemViewModel> Items { get; }

    /// <summary>
    /// Free space before and after each action of this session, by disk (D73): the detail of
    /// the calculation lives here, folded, never on « Stockage ».
    /// </summary>
    public IReadOnlyList<SessionStorageRow> StorageRows { get; }

    /// <summary>The session has a storage measure.</summary>
    public bool HasStorageRows => StorageRows.Count > 0;

    /// <summary>At least one item can still be restored.</summary>
    public bool CanRestore => Items.Any(i => i.IsRestorable);
}

/// <summary>History of sessions: restore all or some items, purge (EF-18 to EF-20).</summary>
public sealed partial class HistoryViewModel : ObservableObject
{
    private readonly IEngineService _engine;
    private readonly IDialogService _dialogs;
    private readonly ISettingsService _settings;
    private readonly INavigationService _navigation;
    private readonly IStorageService? _storage;

    /// <summary>Creates the ViewModel.</summary>
    public HistoryViewModel(IEngineService engine, IDialogService dialogs, ISettingsService settings, INavigationService navigation, IStorageService? storage = null)
    {
        _storage = storage;
        _engine = engine;
        _dialogs = dialogs;
        _settings = settings;
        _navigation = navigation;
    }

    /// <summary>Sessions, most recent first.</summary>
    public ObservableCollection<SessionViewModel> Sessions { get; } = [];

    /// <summary>No session yet (design § 5: « Aucune session pour l'instant. »).</summary>
    public bool IsEmpty => !IsLoading && Sessions.Count == 0;

    /// <summary>Empty History: opens the Orphans page and starts a scan (design § 5).</summary>
    [RelayCommand]
    private void AnalyzeLeftovers() => _navigation.OpenOrphans(startScan: true);

    /// <summary>True while loading.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsEmpty))]
    private bool _isLoading;

    /// <summary>Last operation message is shown.</summary>
    [ObservableProperty]
    private bool _showMessage;

    /// <summary>Items restored by the last restore.</summary>
    [ObservableProperty]
    private int _restoredCount;

    /// <summary>Items that could not be restored by the last restore.</summary>
    [ObservableProperty]
    private int _restoreFailures;

    /// <summary>Space freed by the last purge.</summary>
    [ObservableProperty]
    private string _purgedText = string.Empty;

    /// <summary>True when the last operation was a simulation.</summary>
    [ObservableProperty]
    private bool _lastWasSimulated;

    /// <summary>True when the last operation was a purge.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowRestoreMessage))]
    private bool _lastWasPurge;

    /// <summary>The message is about a restore.</summary>
    public bool ShowRestoreMessage => !LastWasPurge;

    /// <summary>True when the engine failed.</summary>
    [ObservableProperty]
    private bool _hasError;

    /// <summary>Details of the last error.</summary>
    [ObservableProperty]
    private string _errorDetails = string.Empty;

    /// <summary>Loads the sessions.</summary>
    [RelayCommand]
    private async Task LoadAsync(CancellationToken cancellationToken)
    {
        IsLoading = true;
        try
        {
            IReadOnlyList<Session> sessions = await _engine.ListSessionsAsync(cancellationToken);
            IReadOnlyList<StorageEntry> journal = _storage?.Journal() ?? [];
            Sessions.Clear();
            foreach (Session session in sessions)
            {
                Sessions.Add(new SessionViewModel(session, _settings.QuarantineDays, journal));
            }

            HasError = false;
        }
        catch (EngineException ex)
        {
            HasError = true;
            ErrorDetails = ex.Message;
        }
        finally
        {
            IsLoading = false;
        }
    }

    /// <summary>Restores the checked items of a session, or all of them when none is checked (EF-18).</summary>
    [RelayCommand]
    private async Task RestoreAsync(SessionViewModel? session)
    {
        if (session is null)
        {
            return;
        }

        List<string> ids = session.Items.Where(i => i.IsChecked && i.IsRestorable).Select(i => i.Item.Id).ToList();
        var request = new RestoreRequest(session.Session.Id, ids.Count > 0 ? ids : null, DryRun: false);
        try
        {
            Session result = await _engine.RestoreAsync(request, CancellationToken.None);
            int before = session.Items.Count(i => i.Item.Status == QuarantineStatus.Restored);
            RestoredCount = result.Items.Count(i => i.Status == QuarantineStatus.Restored) - before;
            RestoreFailures = result.Errors.Count;
            LastWasSimulated = result.Simulated;
            LastWasPurge = false;
            ShowMessage = true;
            await LoadAsync(CancellationToken.None);
        }
        catch (EngineException ex)
        {
            HasError = true;
            ErrorDetails = ex.Message;
        }
    }

    /// <summary>Deletes for good the sessions older than the configured delay (EF-19).</summary>
    [RelayCommand]
    private async Task PurgeAsync()
    {
        if (!await _dialogs.ConfirmPurgeAsync(_settings.QuarantineDays))
        {
            return;
        }

        try
        {
            var request = new PurgeRequest((uint)_settings.QuarantineDays, DryRun: false);
            PurgeResult result = await _engine.PurgeAsync(request, CancellationToken.None);
            PurgedText = Formats.Size(result.FreedBytes);
            LastWasSimulated = result.Simulated;
            LastWasPurge = true;
            ShowMessage = true;
            await LoadAsync(CancellationToken.None);
        }
        catch (EngineException ex)
        {
            HasError = true;
            ErrorDetails = ex.Message;
        }
    }
}
