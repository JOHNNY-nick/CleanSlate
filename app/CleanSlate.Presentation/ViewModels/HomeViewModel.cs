using System.Collections.ObjectModel;
using CleanSlate.Interop;
using CleanSlate.Presentation.Services;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace CleanSlate.Presentation.ViewModels;

/// <summary>Dashboard: applications, last scan, recoverable space, recent sessions (EF-20, ENF-04).</summary>
public sealed partial class HomeViewModel : ObservableObject
{
    private readonly IEngineService _engine;
    private readonly ISettingsService? _settings;
    private readonly INavigationService? _navigation;
    private readonly OrphansViewModel? _orphans;

    /// <summary>Creates the ViewModel.</summary>
    public HomeViewModel(
        IEngineService engine,
        ISettingsService? settings = null,
        INavigationService? navigation = null,
        OrphansViewModel? orphans = null,
        bool isElevated = true)
    {
        _engine = engine;
        _settings = settings;
        _navigation = navigation;
        _orphans = orphans;
        IsElevated = isElevated;
    }

    /// <summary>Engine version, e.g. <c>0.1.0</c>; empty until loaded.</summary>
    [ObservableProperty]
    private string _engineVersion = string.Empty;

    /// <summary>True when the engine could not be reached.</summary>
    [ObservableProperty]
    private bool _hasError;

    /// <summary>Technical details of the last error, shown under the message.</summary>
    [ObservableProperty]
    private string _errorDetails = string.Empty;

    /// <summary>Visible installed applications.</summary>
    [ObservableProperty]
    private int _appCount;

    /// <summary>Space found by the last scan, or « — ».</summary>
    [ObservableProperty]
    private string _recoverableText = Formats.Unknown;

    /// <summary>Date of the last scan, or « — ».</summary>
    [ObservableProperty]
    private string _lastScanText = Formats.Unknown;

    /// <summary>Space freed by all the sessions.</summary>
    [ObservableProperty]
    private string _freedText = Formats.Size(0);

    /// <summary>The 3 most recent sessions.</summary>
    public ObservableCollection<SessionViewModel> RecentSessions { get; } = [];

    /// <summary>No session yet.</summary>
    public bool HasNoSession => RecentSessions.Count == 0;


    /// <summary>Started with administrator rights (design § 5).</summary>
    public bool IsElevated { get; }

    /// <summary>Not elevated: Program Files and HKLM cannot be analysed correctly.</summary>
    public bool NeedsElevation => !IsElevated;

    /// <summary>Loads every figure of the dashboard.</summary>
    [RelayCommand]
    private async Task LoadAsync(CancellationToken cancellationToken)
    {
        try
        {
            EngineVersion = await _engine.GetVersionAsync(cancellationToken);
            HasError = false;
            ErrorDetails = string.Empty;
            IReadOnlyList<InstalledApp> apps = await _engine.ListAppsAsync(cancellationToken);
            AppCount = apps.Count(a => a.HiddenReason is null);
            IReadOnlyList<Session> sessions = await _engine.ListSessionsAsync(cancellationToken);
            RecentSessions.Clear();
            foreach (Session session in sessions.Take(3))
            {
                RecentSessions.Add(new SessionViewModel(session, _settings?.QuarantineDays ?? 30));
            }

            FreedText = Formats.Size(sessions.Aggregate(0UL, (sum, s) => sum + s.FreedBytes));
            OnPropertyChanged(nameof(HasNoSession));
        }
        catch (Exception ex) when (ex is not OperationCanceledException)
        {
            HasError = true;
            ErrorDetails = ex.Message;
        }

        if (_orphans is { HasScanned: true })
        {
            RecoverableText = _orphans.TotalText;
            LastScanText = _orphans.ScanDateText;
        }


    }

    /// <summary>« Analyser les restes » (design § 4.1, step 1).</summary>
    [RelayCommand]
    private void AnalyzeLeftovers() => _navigation?.OpenOrphans(startScan: true);
}
