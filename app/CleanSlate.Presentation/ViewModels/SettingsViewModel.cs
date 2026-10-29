using System.Collections.ObjectModel;
using CleanSlate.Presentation.Services;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace CleanSlate.Presentation.ViewModels;

/// <summary>Settings: simulation, quarantine delay, exclusions, theme, logs, about (EF-19, EF-20, ENF-08, ENF-10).</summary>
public sealed partial class SettingsViewModel : ObservableObject
{
    private readonly ISettingsService _settings;
    private readonly IEngineService? _engine;

    /// <summary>Creates the ViewModel from the stored settings.</summary>
    public SettingsViewModel(ISettingsService settings, IEngineService? engine = null)
    {
        _settings = settings;
        _engine = engine;
        _simulationMode = settings.SimulationMode;
        _quarantineDays = settings.QuarantineDays;
        _themeIndex = (int)settings.Theme;
        Exclusions = new ObservableCollection<string>(settings.Exclusions);
    }

    /// <summary>Simulation mode (on by default): nothing is moved while it is on.</summary>
    [ObservableProperty]
    private bool _simulationMode;

    /// <summary>Days before the quarantine is purged (1 to 365).</summary>
    [ObservableProperty]
    private double _quarantineDays;

    /// <summary>Theme: 0 follows Windows, 1 light, 2 dark.</summary>
    [ObservableProperty]
    private int _themeIndex;

    /// <summary>Folder or key being typed in the exclusion box.</summary>
    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(AddExclusionCommand))]
    private string _newExclusion = string.Empty;

    /// <summary>Engine version shown in « À propos ».</summary>
    [ObservableProperty]
    private string _engineVersion = string.Empty;

    /// <summary>Folders and keys never proposed (EF-20).</summary>
    public ObservableCollection<string> Exclusions { get; }

    /// <summary>Folder of the log files (ENF-10).</summary>
    public string LogsFolder => _settings.LogsFolder;

    /// <summary>The typed exclusion is an absolute path or a HKCU / HKLM key.</summary>
    public static bool IsValidExclusion(string text)
    {
        string value = text.Trim();
        return value.Length > 3
            && (Path.IsPathFullyQualified(value)
                || value.StartsWith(@"HKCU\", StringComparison.OrdinalIgnoreCase)
                || value.StartsWith(@"HKLM\", StringComparison.OrdinalIgnoreCase));
    }

    /// <summary>Loads the engine version.</summary>
    [RelayCommand]
    private async Task LoadAsync(CancellationToken cancellationToken)
    {
        if (_engine is not null)
        {
            try
            {
                EngineVersion = await _engine.GetVersionAsync(cancellationToken);
            }
            catch (Interop.EngineException)
            {
                EngineVersion = "?";
            }
        }
    }

    /// <summary>Adds the typed exclusion.</summary>
    [RelayCommand(CanExecute = nameof(CanAddExclusion))]
    private void AddExclusion()
    {
        _settings.AddExclusion(NewExclusion);
        Exclusions.Clear();
        foreach (string exclusion in _settings.Exclusions)
        {
            Exclusions.Add(exclusion);
        }

        NewExclusion = string.Empty;
    }

    /// <summary>Removes an exclusion.</summary>
    [RelayCommand]
    private void RemoveExclusion(string? location)
    {
        if (location is null)
        {
            return;
        }

        _settings.RemoveExclusion(location);
        Exclusions.Remove(location);
    }

    private bool CanAddExclusion() => IsValidExclusion(NewExclusion);

    partial void OnSimulationModeChanged(bool value) => _settings.SimulationMode = value;

    partial void OnThemeIndexChanged(int value) => _settings.Theme = Enum.IsDefined(typeof(AppTheme), value) ? (AppTheme)value : AppTheme.System;

    partial void OnQuarantineDaysChanged(double value)
    {
        int days = double.IsNaN(value) ? 30 : (int)Math.Clamp(Math.Round(value), 1, 365);
        _settings.QuarantineDays = days;
        if (Math.Abs(days - value) > 0.001)
        {
            QuarantineDays = days;
        }
    }
}
