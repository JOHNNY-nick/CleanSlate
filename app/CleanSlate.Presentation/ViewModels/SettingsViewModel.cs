using CleanSlate.Presentation.Services;
using CommunityToolkit.Mvvm.ComponentModel;

namespace CleanSlate.Presentation.ViewModels;

/// <summary>Settings: simulation mode and quarantine delay (J4; the rest in J6).</summary>
public sealed partial class SettingsViewModel : ObservableObject
{
    private readonly ISettingsService _settings;

    /// <summary>Creates the ViewModel from the stored settings.</summary>
    public SettingsViewModel(ISettingsService settings)
    {
        _settings = settings;
        _simulationMode = settings.SimulationMode;
        _quarantineDays = settings.QuarantineDays;
    }

    /// <summary>Simulation mode (on by default): nothing is moved while it is on.</summary>
    [ObservableProperty]
    private bool _simulationMode;

    /// <summary>Days before the quarantine is purged (1 to 365).</summary>
    [ObservableProperty]
    private double _quarantineDays;

    partial void OnSimulationModeChanged(bool value) => _settings.SimulationMode = value;

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
