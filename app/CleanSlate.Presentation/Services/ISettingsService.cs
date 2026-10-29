namespace CleanSlate.Presentation.Services;

/// <summary>User settings (Réglages page).</summary>
public interface ISettingsService
{
    /// <summary>Simulation mode: nothing is moved. On by default (specs § 9.2).</summary>
    bool SimulationMode { get; set; }

    /// <summary>Days before the quarantine is purged (EF-19), 1 to 365, 30 by default.</summary>
    int QuarantineDays { get; set; }

    /// <summary>Raised after a setting changed.</summary>
    event EventHandler? Changed;
}
