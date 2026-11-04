using System.Collections.ObjectModel;
using System.ComponentModel;
using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace CleanSlate.Presentation.ViewModels;

/// <summary>
/// Scan of orphan leftovers without a name (EF-05 to EF-11), then quarantine
/// or immediate deletion of the checked items (EF-16, EF-17, D66).
/// </summary>
public sealed partial class OrphansViewModel : ObservableObject
{
    private readonly IEngineService _engine;
    private readonly IDialogService _dialogs;
    private readonly ISettingsService _settings;
    private readonly INavigationService _navigation;

    /// <summary>Creates the ViewModel.</summary>
    public OrphansViewModel(IEngineService engine, IDialogService dialogs, ISettingsService settings, INavigationService navigation)
    {
        _engine = engine;
        _dialogs = dialogs;
        _settings = settings;
        _navigation = navigation;
        _settings.Changed += (_, _) => OnPropertyChanged(nameof(ActionKey));
    }

    /// <summary>Label of the action button: « Mettre en quarantaine » or « Supprimer définitivement » (D67).</summary>
    public string ActionKey => _settings.DeleteImmediately ? "DeleteForGood" : "Quarantine";

    /// <summary>Something was really moved: the success line is shown (D67).</summary>
    public bool HasCleaned => CleanedCount > 0;

    /// <summary>Result of the last quarantine is shown.</summary>
    [ObservableProperty]
    private bool _showCleanResult;

    /// <summary>Items moved to quarantine by the last session.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasCleaned))]
    private int _cleanedCount;

    /// <summary>Size moved to quarantine by the last session.</summary>
    [ObservableProperty]
    private string _cleanedSizeText = string.Empty;

    /// <summary>Locked items that will be moved at the next reboot.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasPendingReboot))]
    private int _pendingRebootCount;

    /// <summary>Items that could not be quarantined (protected, in use…).</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasCleanFailures))]
    private int _cleanFailureCount;

    /// <summary>Some locked items are scheduled for the next reboot.</summary>
    public bool HasPendingReboot => PendingRebootCount > 0;

    /// <summary>Some items were left in place.</summary>
    public bool HasCleanFailures => CleanFailureCount > 0;

    /// <summary>The last quarantine was deleted for good at once (D66).</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CleanedFormatKey))]
    private bool _deletedForGood;

    /// <summary>Result line: « mis en quarantaine » or « supprimés définitivement » (D66).</summary>
    public string CleanedFormatKey => DeletedForGood ? "Format_Deleted" : "Format_Cleaned";

    /// <summary>Groups of the last scan, largest first.</summary>
    public ObservableCollection<LeftoverGroupViewModel> Groups { get; } = [];

    /// <summary>Groups shown by the list: those with an item at the chosen confidence (D69).</summary>
    public ObservableCollection<LeftoverGroupViewModel> VisibleGroups { get; } = [];

    /// <summary>True while a scan runs.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowIntro), nameof(ShowEmpty))]
    private bool _isScanning;

    /// <summary>Progress, 0 to 100.</summary>
    [ObservableProperty]
    private double _progressPercent;

    /// <summary>True while candidates are still being listed (no total yet).</summary>
    [ObservableProperty]
    private bool _isProgressIndeterminate = true;

    /// <summary>Folder or key being analyzed.</summary>
    [ObservableProperty]
    private string _progressPath = string.Empty;

    /// <summary>Progress read by Narrator, updated at most every 2 s (design § 6).</summary>
    [ObservableProperty]
    private string _progressAnnouncement = string.Empty;

    private DateTime _lastAnnouncement = DateTime.MinValue;

    /// <summary>True once a scan has finished (even cancelled).</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowIntro), nameof(ShowEmpty))]
    private bool _hasScanned;

    /// <summary>The last scan was cancelled: results are partial.</summary>
    [ObservableProperty]
    private bool _wasCancelled;

    /// <summary>Summary line: date of the last scan.</summary>
    [ObservableProperty]
    private string _scanDateText = string.Empty;

    /// <summary>Summary line: duration in seconds.</summary>
    [ObservableProperty]
    private long _durationSeconds;

    /// <summary>Summary line: total size found.</summary>
    [ObservableProperty]
    private string _totalText = string.Empty;

    /// <summary>Items that could not be read (access denied…).</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasItemErrors))]
    private int _errorCount;

    /// <summary>Some items could not be read; they were ignored (ENF-09).</summary>
    public bool HasItemErrors => ErrorCount > 0;

    /// <summary>Paths of the items that could not be read (message if no path), shown by « Détails ».</summary>
    public ObservableCollection<string> ItemErrors { get; } = [];

    /// <summary>The list of ignored items is shown (design § 5, « Détails »).</summary>
    [ObservableProperty]
    private bool _showItemErrors;

    /// <summary>True when the engine failed.</summary>
    [ObservableProperty]
    private bool _hasError;

    /// <summary>Details of the last engine error.</summary>
    [ObservableProperty]
    private string _errorDetails = string.Empty;

    /// <summary>Confidence filter: 0 all, 1 High, 2 Medium, 3 Low.</summary>
    [ObservableProperty]
    private int _filterIndex;

    /// <summary>Number of checked items.</summary>
    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(QuarantineCommand))]
    private int _selectedCount;

    /// <summary>Size of the checked items.</summary>
    [ObservableProperty]
    private string _selectedSizeText = Formats.Size(0);


    /// <summary>Number of groups.</summary>
    public int GroupCount => Groups.Count;

    /// <summary>Nothing scanned yet: invitation to start.</summary>
    public bool ShowIntro => !HasScanned && !IsScanning;

    /// <summary>Scan finished without any leftover.</summary>
    public bool ShowEmpty => HasScanned && !IsScanning && Groups.Count == 0;

    /// <summary>Runs a read-only scan; cancelling keeps the partial results.</summary>
    [RelayCommand(IncludeCancelCommand = true)]
    private async Task ScanAsync(CancellationToken cancellationToken)
    {
        IsScanning = true;
        IsProgressIndeterminate = true;
        ProgressPercent = 0;
        ProgressPath = string.Empty;
        HasError = false;
        var progress = new Progress<ScanProgress>(OnProgress);
        try
        {
            ScanReport report = await _engine.ScanOrphansAsync(new ScanOptions(), progress, cancellationToken);
            Show(report);
        }
        catch (Exception ex) when (ex is not OperationCanceledException)
        {
            HasError = true;
            ErrorDetails = ex.Message;
        }
        finally
        {
            IsScanning = false;
        }
    }


    /// <summary>Progress bar of the quarantine (task 2).</summary>
    public QuarantineProgressViewModel CleanProgress { get; } = new();

    /// <summary>
    /// Moves the checked items to quarantine after the mandatory confirmation
    /// (EF-16, EF-17), or deletes them for good when the user chose it (D66).
    /// </summary>
    [RelayCommand(CanExecute = nameof(HasSelection))]
    private async Task QuarantineAsync(CancellationToken cancellationToken)
    {
        List<LeftoverItemViewModel> selected = Groups.SelectMany(g => g.Items).Where(i => i.IsChecked).ToList();
        var summary = new CleanSummary(
            selected.Count,
            SelectedSizeText,
            selected.GroupBy(i => i.Kind).Select(g => new KeyValuePair<LeftoverKind, int>(g.Key, g.Count())).ToList(),
            _settings.QuarantineDays,
            _settings.DeleteImmediately);
        CleanDecision decision = await _dialogs.ConfirmCleanAsync(summary);
        if (!decision.Confirmed)
        {
            return;
        }

        // Always real: the application has no simulation mode (D66).
        var plan = new CleanPlan(selected.Select(i => i.Leftover.Id).ToList(), Origin.Orphans, DryRun: false);
        IProgress<CleanProgress> progress = CleanProgress.Start();
        try
        {
            Session session;
            try
            {
                session = await _engine.CleanAsync(plan, progress, cancellationToken);
            }
            catch (EngineException ex) when (ex.Code == "CONFIRMATION_REQUIRED")
            {
                CleanDecision again = await _dialogs.ConfirmCleanAsync(summary with { RequiresWord = true });
                if (!again.Confirmed)
                {
                    return;
                }

                session = await _engine.CleanAsync(plan with { Confirmation = again.Word }, progress, cancellationToken);
            }

            DeletedForGood = await ImmediateDeletion.ApplyAsync(_engine, _settings, session, cancellationToken);
            ShowCleanOutcome(session);
        }
        catch (EngineException ex)
        {
            HasError = true;
            ErrorDetails = ex.Message;
        }
        finally
        {
            CleanProgress.Stop();
        }
    }

    private bool HasSelection() => SelectedCount > 0;

    /// <summary>Nothing to clean: back to Home (design § 5).</summary>
    [RelayCommand]
    private void GoHome() => _navigation.OpenHome();

    /// <summary>Locked items: their session is in History (design § 5, « Voir les fichiers »).</summary>
    [RelayCommand]
    private void OpenHistory() => _navigation.OpenHistory();


    /// <summary>Shows or hides the ignored items (design § 5, « Détails »).</summary>
    [RelayCommand]
    private void ToggleItemErrors() => ShowItemErrors = !ShowItemErrors;

    private void ShowCleanOutcome(Session session)
    {
        if (session.Simulated)
        {
            return; // nothing was moved (the application never asks for a simulation, D66)
        }

        var moved = session.Items
            .Where(i => i.Status is QuarantineStatus.Quarantined or QuarantineStatus.PendingReboot)
            .Select(i => i.Id)
            .ToHashSet();
        CleanedCount = session.Items.Count(i => i.Status == QuarantineStatus.Quarantined);
        CleanedSizeText = Formats.Size(session.FreedBytes);
        PendingRebootCount = session.Items.Count(i => i.Status == QuarantineStatus.PendingReboot);
        CleanFailureCount = session.Items.Count(i => i.Status == QuarantineStatus.Failed);
        // The groups' own lists are never changed once shown: removing an item from an
        // ItemsRepeater inside an Expander crashed WinUI (COMException 0x80004005, D70).
        // Moved items are hidden for good; a group left empty leaves the top-level lists.
        foreach (LeftoverGroupViewModel group in Groups.ToList())
        {
            foreach (LeftoverItemViewModel item in group.Items.Where(i => moved.Contains(i.Leftover.Id)))
            {
                item.PropertyChanged -= OnItemChanged;
                item.IsChecked = false;
                item.IsDone = true;
                item.IsVisible = false;
            }

            if (group.Items.All(i => i.IsDone))
            {
                Groups.Remove(group);
                VisibleGroups.Remove(group);
            }
        }

        ShowCleanResult = true;
        UpdateSelection();
        OnPropertyChanged(nameof(GroupCount));
        OnPropertyChanged(nameof(ShowEmpty));
    }

    private void OnProgress(ScanProgress value)
    {
        IsProgressIndeterminate = value.Total == 0;
        ProgressPercent = value.Total == 0 ? 0 : Math.Min(100, 100.0 * value.Current / value.Total);
        ProgressPath = value.Path;
        if (value.Total > 0 && DateTime.UtcNow - _lastAnnouncement >= TimeSpan.FromSeconds(2))
        {
            _lastAnnouncement = DateTime.UtcNow;
            ProgressAnnouncement = $"{Math.Round(ProgressPercent)} %";
        }
    }

    private void Show(ScanReport report)
    {
        foreach (LeftoverGroupViewModel old in Groups)
        {
            foreach (LeftoverItemViewModel item in old.Items)
            {
                item.PropertyChanged -= OnItemChanged;
            }
        }

        Groups.Clear();
        foreach (LeftoverGroup group in report.Groups)
        {
            var vm = new LeftoverGroupViewModel(group);
            foreach (LeftoverItemViewModel item in vm.Items)
            {
                item.PropertyChanged += OnItemChanged;
            }

            Groups.Add(vm);
        }

        HasScanned = true;
        WasCancelled = report.Cancelled;
        ErrorCount = report.Errors.Count;
        ItemErrors.Clear();
        foreach (ItemError error in report.Errors)
        {
            // The engine message repeats the path, in English: the path alone is enough.
            ItemErrors.Add(error.Path ?? error.Message);
        }

        ShowItemErrors = false;
        ScanDateText = Formats.Date(DateOnly.FromDateTime(DateTime.Now));
        DurationSeconds = (long)Math.Round(report.DurationMs / 1000.0);
        TotalText = Formats.Size(report.Groups.Aggregate(0UL, (sum, g) => sum + g.TotalBytes));
        ApplyFilter();
        UpdateSelection();
        OnPropertyChanged(nameof(GroupCount));
        OnPropertyChanged(nameof(ShowEmpty));
    }

    partial void OnFilterIndexChanged(int value) => ApplyFilter();

    private void ApplyFilter()
    {
        Confidence? wanted = FilterIndex switch
        {
            1 => Confidence.High,
            2 => Confidence.Medium,
            3 => Confidence.Low,
            _ => null,
        };
        foreach (LeftoverGroupViewModel group in Groups)
        {
            foreach (LeftoverItemViewModel item in group.Items)
            {
                item.IsVisible = !item.IsDone && (wanted is null || item.Confidence == wanted);
            }

            group.IsVisible = group.Items.Any(i => i.IsVisible);
        }

        // The list binds to the visible groups only: a hidden group would
        // still take an empty slot in the ListView (D69).
        VisibleGroups.Clear();
        foreach (LeftoverGroupViewModel group in Groups.Where(g => g.IsVisible))
        {
            VisibleGroups.Add(group);
        }
    }

    private void OnItemChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(LeftoverItemViewModel.IsChecked))
        {
            UpdateSelection();
        }
    }

    private void UpdateSelection()
    {
        var checkedItems = Groups.SelectMany(g => g.Items).Where(i => i.IsChecked).ToList();
        SelectedCount = checkedItems.Count;
        SelectedSizeText = Formats.Size(checkedItems.Aggregate(0UL, (sum, i) => sum + i.Leftover.SizeBytes));
    }
}
