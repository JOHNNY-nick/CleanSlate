using System.Collections.ObjectModel;
using System.ComponentModel;
using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace CleanSlate.Presentation.ViewModels;

/// <summary>
/// Scan of orphan leftovers without a name (EF-05 to EF-11). In J3 the
/// action only simulates: nothing is moved (ENF-04).
/// </summary>
public sealed partial class OrphansViewModel : ObservableObject
{
    private readonly IEngineService _engine;

    /// <summary>Creates the ViewModel.</summary>
    public OrphansViewModel(IEngineService engine)
    {
        _engine = engine;
    }

    /// <summary>Groups of the last scan, largest first.</summary>
    public ObservableCollection<LeftoverGroupViewModel> Groups { get; } = [];

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
    [NotifyCanExecuteChangedFor(nameof(SimulateCommand))]
    private int _selectedCount;

    /// <summary>Size of the checked items.</summary>
    [ObservableProperty]
    private string _selectedSizeText = Formats.Size(0);

    /// <summary>Simulation result shown under the list (design § 5).</summary>
    [ObservableProperty]
    private bool _showSimulation;

    /// <summary>Number of items of the last simulation.</summary>
    [ObservableProperty]
    private int _simulatedCount;

    /// <summary>Size of the last simulation.</summary>
    [ObservableProperty]
    private string _simulatedSizeText = string.Empty;

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
        ShowSimulation = false;
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

    /// <summary>Shows what would be quarantined, without touching anything (ENF-04).</summary>
    [RelayCommand(CanExecute = nameof(HasSelection))]
    private void Simulate()
    {
        SimulatedCount = SelectedCount;
        SimulatedSizeText = SelectedSizeText;
        ShowSimulation = true;
    }

    /// <summary>Real quarantine: enabled in J4.</summary>
    [RelayCommand(CanExecute = nameof(CanQuarantine))]
    private void Quarantine()
    {
    }

    private bool HasSelection() => SelectedCount > 0;

    private bool CanQuarantine() => false;

    private void OnProgress(ScanProgress value)
    {
        IsProgressIndeterminate = value.Total == 0;
        ProgressPercent = value.Total == 0 ? 0 : Math.Min(100, 100.0 * value.Current / value.Total);
        ProgressPath = value.Path;
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
                item.IsVisible = wanted is null || item.Confidence == wanted;
            }

            group.IsVisible = group.Items.Any(i => i.IsVisible);
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
        ShowSimulation = false;
    }
}
