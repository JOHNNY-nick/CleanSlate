using System.Text.Json;
using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;

namespace CleanSlate.App.Services;

/// <summary>
/// Settings stored in <c>%LOCALAPPDATA%\CleanSlate\settings.json</c>. The application
/// has no simulation mode (D66): the first-launch acceptance comes before any action.
/// </summary>
public sealed class SettingsService : ISettingsService
{
    private static readonly string Folder = DevMode.SettingsFolder;

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
    public bool DeleteImmediately
    {
        get => _stored.DeleteImmediately;
        set => Update(_stored with { DeleteImmediately = value });
    }

    /// <inheritdoc/>
    public int QuarantineDays
    {
        get => _stored.QuarantineDays;
        set => Update(_stored with { QuarantineDays = Math.Clamp(value, 1, 365) });
    }

    /// <inheritdoc/>
    public bool FirstRunDone
    {
        get => _stored.FirstRunDone;
        set => Update(_stored with { FirstRunDone = value });
    }

    /// <inheritdoc/>
    public string? QuarantineFolder
    {
        get => _stored.QuarantineFolder;
        set => Update(_stored with { QuarantineFolder = value });
    }

    /// <inheritdoc/>
    public string? ExportFolder
    {
        get => _stored.ExportFolder;
        set => Update(_stored with { ExportFolder = value });
    }

    /// <inheritdoc/>
    public AppTheme Theme
    {
        get => _stored.Theme;
        set => Update(_stored with { Theme = value });
    }

    /// <inheritdoc/>
    public PaintStyle PaintStyle
    {
        get => _stored.PaintStyle;
        set => Update(_stored with { PaintStyle = value });
    }

    /// <inheritdoc/>
    public bool PaintDark
    {
        get => _stored.PaintDark;
        set => Update(_stored with { PaintDark = value });
    }

    /// <inheritdoc/>
    public bool ExpertMode
    {
        get => _stored.ExpertMode;
        set => Update(_stored with { ExpertMode = value });
    }

    /// <inheritdoc/>
    public int AlertPercent
    {
        get => _stored.AlertPercent;
        set => Update(_stored with { AlertPercent = Math.Clamp(value, 1, 50) });
    }

    /// <inheritdoc/>
    public IReadOnlyList<string> Exclusions => _stored.Exclusions;

    /// <inheritdoc/>
    public string LogsFolder => CrashLog.CurrentDirectory();

    /// <inheritdoc/>
    public string SettingsFile => FilePath;

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

    private sealed record Stored(
        bool DeleteImmediately = false,
        int QuarantineDays = 30,
        AppTheme Theme = AppTheme.System,
        bool FirstRunDone = false,
        string? QuarantineFolder = null,
        string? ExportFolder = null,
        PaintStyle PaintStyle = PaintStyle.Splashes,
        bool PaintDark = false,
        bool ExpertMode = false,
        int AlertPercent = 10)
    {
        public IReadOnlyList<string> Exclusions { get; init; } = [];

        public bool Equals(Stored? other) =>
            other is not null
            && DeleteImmediately == other.DeleteImmediately
            && QuarantineDays == other.QuarantineDays
            && Theme == other.Theme
            && FirstRunDone == other.FirstRunDone
            && QuarantineFolder == other.QuarantineFolder
            && ExportFolder == other.ExportFolder
            && PaintStyle == other.PaintStyle
            && PaintDark == other.PaintDark
            && ExpertMode == other.ExpertMode
            && AlertPercent == other.AlertPercent
            && Exclusions.SequenceEqual(other.Exclusions);

        public override int GetHashCode() => HashCode.Combine(DeleteImmediately, QuarantineDays, Theme, FirstRunDone, QuarantineFolder, ExportFolder, Exclusions.Count);
    }
}
