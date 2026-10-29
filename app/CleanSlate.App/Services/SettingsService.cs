using System.Text.Json;
using CleanSlate.Presentation.Services;

namespace CleanSlate.App.Services;

/// <summary>
/// Settings stored in <c>%LOCALAPPDATA%\CleanSlate\settings.json</c>. Simulation
/// mode is on until the user turns it off (specs § 9.2).
/// </summary>
public sealed class SettingsService : ISettingsService
{
    private static readonly string Folder = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "CleanSlate");

    private static readonly string FilePath = Path.Combine(Folder, "settings.json");

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

    /// <inheritdoc/>
    public AppTheme Theme
    {
        get => _stored.Theme;
        set => Update(_stored with { Theme = value });
    }

    /// <inheritdoc/>
    public IReadOnlyList<string> Exclusions => _stored.Exclusions;

    /// <inheritdoc/>
    public string LogsFolder => Path.Combine(Folder, "logs");

    /// <inheritdoc/>
    public void AddExclusion(string location)
    {
        string trimmed = location.Trim().TrimEnd('\\');
        if (trimmed.Length > 0 && !_stored.Exclusions.Contains(trimmed, StringComparer.OrdinalIgnoreCase))
        {
            Update(_stored with { Exclusions = [.. _stored.Exclusions, trimmed] });
        }
    }

    /// <inheritdoc/>
    public void RemoveExclusion(string location) =>
        Update(_stored with { Exclusions = _stored.Exclusions.Where(e => !string.Equals(e, location, StringComparison.OrdinalIgnoreCase)).ToList() });

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
            Directory.CreateDirectory(Folder);
            File.WriteAllText(FilePath, JsonSerializer.Serialize(next));
        }

        Changed?.Invoke(this, EventArgs.Empty);
    }

    private sealed record Stored(bool SimulationMode = true, int QuarantineDays = 30, AppTheme Theme = AppTheme.System)
    {
        public IReadOnlyList<string> Exclusions { get; init; } = [];

        public bool Equals(Stored? other) =>
            other is not null
            && SimulationMode == other.SimulationMode
            && QuarantineDays == other.QuarantineDays
            && Theme == other.Theme
            && Exclusions.SequenceEqual(other.Exclusions);

        public override int GetHashCode() => HashCode.Combine(SimulationMode, QuarantineDays, Theme, Exclusions.Count);
    }
}
