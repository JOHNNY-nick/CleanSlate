using System.Collections.ObjectModel;
using System.ComponentModel;
using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace CleanSlate.Presentation.ViewModels;

/// <summary>
/// Uninstall wizard (design § 4.2): 1 confirmation, 2 official uninstaller,
/// 3 leftovers, 4 summary. The classic mode stops after the uninstaller.
/// </summary>
public sealed partial class UninstallWizardViewModel : ObservableObject
{
    private readonly IEngineService _engine;
    private readonly IDialogService _dialogs;
    private readonly ISettingsService _settings;
    private readonly INavigationService _navigation;

    /// <summary>Creates the ViewModel.</summary>
    public UninstallWizardViewModel(IEngineService engine, IDialogService dialogs, ISettingsService settings, INavigationService navigation)
    {
        _engine = engine;
        _dialogs = dialogs;
        _settings = settings;
        _navigation = navigation;
    }

    /// <summary>Application being uninstalled.</summary>
    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(LaunchCommand))]
    private AppItemViewModel? _app;

    /// <summary>Full uninstall (leftovers too) or classic (uninstaller only).</summary>
    [ObservableProperty]
    private bool _isComplete = true;

    /// <summary>Current step, 1 to 4.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsStep1), nameof(IsStep2), nameof(IsStep3), nameof(IsStep4))]
    private int _step = 1;

    /// <summary>Create a restore point first (EF-15), checked by default.</summary>
    [ObservableProperty]
    private bool _createRestorePoint = true;

    /// <summary>Silent mode (EF-14), when the application offers one.</summary>
    [ObservableProperty]
    private bool _quiet;

    /// <summary>Last uninstaller result.</summary>
    [ObservableProperty]
    private UninstallResult? _result;

    /// <summary>Error of the last step (uninstaller not found…).</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasError), nameof(IsUninstallerMissing), nameof(ShowGenericError))]
    [NotifyCanExecuteChangedFor(nameof(SearchLeftoversAnywayCommand))]
    private string _errorCode = string.Empty;

    /// <summary>Leftovers searched without running the uninstaller, which is missing (D45).</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowUninstalledBanner), nameof(ShowCommandLine), nameof(ShowCleanedLine), nameof(ShowCleanSimulatedLine))]
    private bool _withoutUninstaller;

    /// <summary>The leftover search found nothing (step 4).</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowCleanedLine), nameof(ShowCleanSimulatedLine))]
    private bool _noLeftoverFound;

    /// <summary>Details of the error.</summary>
    [ObservableProperty]
    private string _errorDetails = string.Empty;

    /// <summary>Items moved by the quarantine of the leftovers.</summary>
    [ObservableProperty]
    private int _cleanedCount;

    /// <summary>Size freed by the quarantine.</summary>
    [ObservableProperty]
    private string _cleanedSizeText = Formats.Size(0);

    /// <summary>The quarantine of the leftovers was only simulated.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowCleanedLine), nameof(ShowCleanSimulatedLine))]
    private bool _cleanSimulated;

    /// <summary>Number of checked leftovers.</summary>
    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(QuarantineCommand))]
    private int _selectedCount;

    /// <summary>Size of the checked leftovers.</summary>
    [ObservableProperty]
    private string _selectedSizeText = Formats.Size(0);

    /// <summary>Leftovers found after the uninstaller (step 3).</summary>
    public ObservableCollection<LeftoverGroupViewModel> Groups { get; } = [];

    /// <summary>Step 1 is shown.</summary>
    public bool IsStep1 => Step == 1;

    /// <summary>Step 2 is shown.</summary>
    public bool IsStep2 => Step == 2;

    /// <summary>Step 3 is shown.</summary>
    public bool IsStep3 => Step == 3;

    /// <summary>Step 4 is shown.</summary>
    public bool IsStep4 => Step == 4;

    /// <summary>An error is shown.</summary>
    public bool HasError => ErrorCode.Length > 0;

    /// <summary>The application has a silent uninstaller (EF-14).</summary>
    public bool CanQuiet => App?.App is { } a && (a.QuietUninstallString is not null || a.IsMsi);

    /// <summary>Simulation mode of the settings.</summary>
    public bool IsSimulationMode => _settings.SimulationMode;

    /// <summary>The uninstaller was cancelled or failed: the entry is still there (design § 4.2).</summary>
    public bool StillInstalled => Result is { Simulated: false, StillInstalled: true };

    /// <summary>The uninstaller was only simulated: the command was not run.</summary>
    public bool WasSimulated => Result is { Simulated: true };

    /// <summary>Leftovers were really handled (summary line of step 4).</summary>
    public bool ShowCleanedLine => !NoLeftoverFound
        && (WithoutUninstaller ? !CleanSimulated : Result is { Simulated: false, StillInstalled: false });

    /// <summary>Leftovers found without uninstaller, quarantine only simulated (step 4).</summary>
    public bool ShowCleanSimulatedLine => WithoutUninstaller && !NoLeftoverFound && CleanSimulated;

    /// <summary>The uninstaller program is missing (design § 5): leftovers can be searched anyway.</summary>
    public bool IsUninstallerMissing => ErrorCode == "NO_UNINSTALLER";

    /// <summary>Any other error.</summary>
    public bool ShowGenericError => HasError && !IsUninstallerMissing;

    /// <summary>Step 3 banner « L'application a été désinstallée. ».</summary>
    public bool ShowUninstalledBanner => !WithoutUninstaller;

    /// <summary>The uninstaller command is shown in the summary.</summary>
    public bool ShowCommandLine => !WithoutUninstaller;

    /// <summary>Starts the wizard for an application.</summary>
    public void Start(InstalledApp app, bool complete)
    {
        App = new AppItemViewModel(app);
        IsComplete = complete;
        Step = 1;
        CreateRestorePoint = true;
        Quiet = false;
        Result = null;
        ErrorCode = string.Empty;
        ErrorDetails = string.Empty;
        CleanedCount = 0;
        CleanSimulated = false;
        WithoutUninstaller = false;
        NoLeftoverFound = false;
        ClearGroups();
        OnPropertyChanged(nameof(CanQuiet));
        OnPropertyChanged(nameof(IsSimulationMode));
        OnPropertyChanged(nameof(StillInstalled));
    }

    /// <summary>Step 1 → 2 → 3 (or 4): runs the uninstaller, then looks for leftovers.</summary>
    [RelayCommand(CanExecute = nameof(HasApp), IncludeCancelCommand = true)]
    private async Task LaunchAsync(CancellationToken cancellationToken)
    {
        if (App is null)
        {
            return;
        }

        ErrorCode = string.Empty;
        Step = 2;
        try
        {
            var request = new UninstallRequest(App.App.Id, Quiet && CanQuiet, CreateRestorePoint, DryRun: _settings.SimulationMode);
            Result = await _engine.UninstallAsync(request, cancellationToken);
            OnPropertyChanged(nameof(StillInstalled));
            OnPropertyChanged(nameof(WasSimulated));
            OnPropertyChanged(nameof(ShowCleanedLine));
            if (!IsComplete || Result.Simulated || Result.StillInstalled)
            {
                Step = 4;
                return;
            }

            ScanReport report = await _engine.ScanAppLeftoversAsync(new AppLeftoversRequest(App.App.Id), null, CancellationToken.None);
            ShowLeftovers(report);
            // No leftover: step 3 is skipped (design § 4.2).
            NoLeftoverFound = Groups.Count == 0;
            Step = NoLeftoverFound ? 4 : 3;
        }
        catch (EngineException ex)
        {
            ErrorCode = ex.Code;
            ErrorDetails = ex.Message;
            Step = 1;
        }
    }

    /// <summary>
    /// The uninstaller is missing: looks for the leftovers anyway (design § 5). The engine
    /// caps them at Medium, so none is prechecked (D45); the Windows entry stays.
    /// </summary>
    [RelayCommand(CanExecute = nameof(IsUninstallerMissing))]
    private async Task SearchLeftoversAnywayAsync()
    {
        if (App is null)
        {
            return;
        }

        ErrorCode = string.Empty;
        ErrorDetails = string.Empty;
        try
        {
            var request = new AppLeftoversRequest(App.App.Id, UninstallerMissing: true);
            ScanReport report = await _engine.ScanAppLeftoversAsync(request, null, CancellationToken.None);
            WithoutUninstaller = true;
            ShowLeftovers(report);
            NoLeftoverFound = Groups.Count == 0;
            Step = NoLeftoverFound ? 4 : 3;
        }
        catch (EngineException ex)
        {
            ErrorCode = ex.Code;
            ErrorDetails = ex.Message;
        }
    }

    /// <summary>Step 3 → 4: quarantines the checked leftovers after confirmation (EF-16, EF-17).</summary>
    [RelayCommand(CanExecute = nameof(HasSelection))]
    private async Task QuarantineAsync()
    {
        if (App is null)
        {
            return;
        }

        List<LeftoverItemViewModel> selected = Groups.SelectMany(g => g.Items).Where(i => i.IsChecked).ToList();
        var summary = new CleanSummary(
            selected.Count,
            SelectedSizeText,
            selected.GroupBy(i => i.Kind).Select(g => new KeyValuePair<LeftoverKind, int>(g.Key, g.Count())).ToList(),
            _settings.QuarantineDays,
            _settings.SimulationMode);
        CleanDecision decision = await _dialogs.ConfirmCleanAsync(summary);
        if (!decision.Confirmed)
        {
            return;
        }

        var plan = new CleanPlan(selected.Select(i => i.Leftover.Id).ToList(), Origin.ForApp(App.App.Id), DryRun: _settings.SimulationMode, Confirmation: decision.Word);
        try
        {
            Session session = await _engine.CleanAsync(plan, CancellationToken.None);
            CleanSimulated = session.Simulated;
            CleanedCount = session.Simulated
                ? selected.Count
                : session.Items.Count(i => i.Status is QuarantineStatus.Quarantined or QuarantineStatus.PendingReboot);
            CleanedSizeText = session.Simulated ? SelectedSizeText : Formats.Size(session.FreedBytes);
            Step = 4;
        }
        catch (EngineException ex)
        {
            ErrorCode = ex.Code;
            ErrorDetails = ex.Message;
        }
    }

    /// <summary>Step 3 → 4 without touching the leftovers.</summary>
    [RelayCommand]
    private void SkipLeftovers() => Step = 4;

    /// <summary>Opens the History page (step 4).</summary>
    [RelayCommand]
    private void OpenHistory() => _navigation.OpenHistory();

    private bool HasApp() => App is not null;

    private bool HasSelection() => SelectedCount > 0;

    private void ShowLeftovers(ScanReport report)
    {
        ClearGroups();
        foreach (LeftoverGroup group in report.Groups)
        {
            var vm = new LeftoverGroupViewModel(group) { IsExpanded = true };
            foreach (LeftoverItemViewModel item in vm.Items)
            {
                item.PropertyChanged += OnItemChanged;
            }

            Groups.Add(vm);
        }

        UpdateSelection();
    }

    private void ClearGroups()
    {
        foreach (LeftoverItemViewModel item in Groups.SelectMany(g => g.Items))
        {
            item.PropertyChanged -= OnItemChanged;
        }

        Groups.Clear();
        UpdateSelection();
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
