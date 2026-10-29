using System.Text.Json;
using CleanSlate.Presentation.Services;

namespace CleanSlate.App.Services;

/// <summary>
/// Settings stored in <c>%LOCALAPPDATA%\CleanSlate\settings.json</c>. Simulation
/// mode is on until the user turns it off (specs § 9.2).
/// </summary>
public sealed class SettingsService : ISettingsService
{
    private static readonly string FilePath = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "CleanSlate", "settings.json");

    private readonly object _gate = new();
    private Stored _stored;

    /// <summary>Loads the stored settings, or the defaults.</summary>
    public SettingsService()
    {
        _stored = Load();
    }

    /// <inheritdoc/>
    public event EventHandler? Changed;

    /// <inheritdoc/>
    public bool SimulationMode
    {
        get => _stored.SimulationMode;
        set => Update(_stored with { SimulationMode = value });
    }

    /// <inheritdoc/>
    public int QuarantineDays
    {
        get => _stored.QuarantineDays;
        set => Update(_stored with { QuarantineDays = Math.Clamp(value, 1, 365) });
    }

    private static Stored Load()
    {
        try
        {
            return File.Exists(FilePath)
                ? JsonSerializer.Deserialize<Stored>(File.ReadAllText(FilePath)) ?? new Stored()
                : new Stored();
        }
        catch (Exception ex) when (ex is IOException or JsonException or UnauthorizedAccessException)
        {
            return new Stored();
        }
    }

    private void Update(Stored next)
    {
        lock (_gate)
        {
            if (next == _stored)
            {
                return;
            }

            _stored = next;
            Directory.CreateDirectory(Path.GetDirectoryName(FilePath)!);
            File.WriteAllText(FilePath, JsonSerializer.Serialize(next));
        }

        Changed?.Invoke(this, EventArgs.Empty);
    }

    private sealed record Stored(bool SimulationMode = true, int QuarantineDays = 30);
}
