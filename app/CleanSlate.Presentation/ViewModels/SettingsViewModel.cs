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
    private readonly ILogExportService? _logExport;
    private readonly IFolderService? _folders;

    /// <summary>Creates the ViewModel from the stored settings.</summary>
    public SettingsViewModel(ISettingsService settings, IEngineService? engine = null, ILogExportService? logExport = null, IFolderService? folders = null)
    {
        _settings = settings;
        _engine = engine;
        _logExport = logExport;
        _folders = folders;
        _quarantineFolder = settings.QuarantineFolder is { Length: > 0 } quarantine ? quarantine : folders?.DefaultQuarantineFolder ?? string.Empty;
        _exportFolder = settings.ExportFolder is { Length: > 0 } export ? export : folders?.DefaultExportFolder ?? string.Empty;
        _deleteImmediately = settings.DeleteImmediately;
        _quarantineDays = settings.QuarantineDays;
        _themeIndex = ThemePalette.IndexOf(settings.Theme);
        _paintStyleIndex = (int)settings.PaintStyle;
        _paintDark = settings.PaintDark;
        Exclusions = new ObservableCollection<string>(settings.Exclusions);
    }

    /// <summary>« Supprimer définitivement, sans quarantaine » (D66), off by default.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(KeepsQuarantine))]
    private bool _deleteImmediately;

    /// <summary>The quarantine delay applies (immediate deletion not chosen).</summary>
    public bool KeepsQuarantine => !DeleteImmediately;

    /// <summary>Days before the quarantine is purged (1 to 365).</summary>
    [ObservableProperty]
    private double _quarantineDays;

    /// <summary>Theme: 0 Système, 1 Clair, 2 Sombre, 3 Multicolore (D72).</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsColorful))]
    private int _themeIndex;

    /// <summary>Multicolore is chosen: its style and background are offered (D76).</summary>
    public bool IsColorful => ThemePalette.FromIndex(ThemeIndex) == AppTheme.Colorful;

    /// <summary>Style of the Multicolore paint: 0 Éclaboussures, 1 Aquarelle, 2 Explosion (D76).</summary>
    [ObservableProperty]
    private int _paintStyleIndex;

    /// <summary>Multicolore on a dark background (D76).</summary>
    [ObservableProperty]
    private bool _paintDark;

    /// <summary>Folder or key being typed in the exclusion box.</summary>
    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(AddExclusionCommand))]
    private string _newExclusion = string.Empty;

    /// <summary>Engine version shown in « À propos ».</summary>
    [ObservableProperty]
    private string _engineVersion = string.Empty;

    /// <summary>Path of the last exported zip of the logs.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasExport))]
    private string _exportedPath = string.Empty;

    /// <summary>The last export failed.</summary>
    [ObservableProperty]
    private bool _exportFailed;

    /// <summary>Reason of the last failed export.</summary>
    [ObservableProperty]
    private string _exportError = string.Empty;

    /// <summary>A zip of the logs was written.</summary>
    public bool HasExport => ExportedPath.Length > 0;

    /// <summary>Folder of the quarantine and its history (D63).</summary>
    [ObservableProperty]
    private string _quarantineFolder;

    /// <summary>Folder of the exported logs (D63).</summary>
    [ObservableProperty]
    private string _exportFolder;

    /// <summary>The quarantine is empty: its folder may move (no session left behind).</summary>
    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(ChangeQuarantineFolderCommand))]
    private bool _canChangeQuarantineFolder;

    /// <summary>Why the last folder chosen was refused.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasFolderProblem))]
    private FolderProblem _folderProblem;

    /// <summary>Reason why a folder could not be created or picked.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasFolderError))]
    private string _folderError = string.Empty;

    /// <summary>The last folder chosen was refused.</summary>
    public bool HasFolderProblem => FolderProblem != FolderProblem.None;

    /// <summary>A folder could not be created or picked.</summary>
    public bool HasFolderError => FolderError.Length > 0;

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

            try
            {
                var sessions = await _engine.ListSessionsAsync(cancellationToken);
                CanChangeQuarantineFolder = _folders is not null
                    && !sessions.Any(s => s.Status is Interop.SessionStatus.Completed or Interop.SessionStatus.Partial);
            }
            catch (Interop.EngineException)
            {
                CanChangeQuarantineFolder = false;
            }
        }
    }

    /// <summary>Moves the quarantine to another folder, only while it is empty (D63).</summary>
    [RelayCommand(CanExecute = nameof(CanChangeQuarantineFolder))]
    private async Task ChangeQuarantineFolderAsync()
    {
        if (_folders is null || await PickAsync() is not string path)
        {
            return;
        }

        FolderProblem = _folders.CheckQuarantineFolder(path);
        if (HasFolderProblem)
        {
            return;
        }

        try
        {
            _folders.PrepareQuarantineFolder(path);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            FolderError = ex.Message;
            return;
        }

        _settings.QuarantineFolder = FolderRules.SamePath(path, _folders.DefaultQuarantineFolder) ? null : path;
        QuarantineFolder = path;
    }

    /// <summary>Chooses the folder of the exported logs.</summary>
    [RelayCommand]
    private async Task ChangeExportFolderAsync()
    {
        if (_folders is null || await PickAsync() is not string path)
        {
            return;
        }

        FolderProblem = _folders.CheckExportFolder(path);
        if (!HasFolderProblem)
        {
            _settings.ExportFolder = FolderRules.SamePath(path, _folders.DefaultExportFolder) ? null : path;
            ExportFolder = path;
        }
    }

    /// <summary>Exports the logs to the Desktop again.</summary>
    [RelayCommand]
    private void UseDefaultExportFolder()
    {
        _settings.ExportFolder = null;
        ExportFolder = _folders?.DefaultExportFolder ?? string.Empty;
        FolderProblem = FolderProblem.None;
    }

    private async Task<string?> PickAsync()
    {
        FolderProblem = FolderProblem.None;
        FolderError = string.Empty;
        try
        {
            return _folders is null ? null : await _folders.PickFolderAsync();
        }
        catch (Exception ex) when (ex is InvalidOperationException or System.Runtime.InteropServices.COMException or UnauthorizedAccessException or NotSupportedException or ArgumentException)
        {
            FolderError = ex.Message;
            return null;
        }
    }

    /// <summary>Writes the zip of the logs and settings, to attach to a problem report.</summary>
    [RelayCommand]
    private async Task ExportLogsAsync(CancellationToken cancellationToken)
    {
        if (_logExport is null)
        {
            return;
        }

        ExportFailed = false;
        ExportError = string.Empty;
        try
        {
            ExportedPath = await _logExport.ExportAsync(cancellationToken);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or InvalidDataException)
        {
            ExportedPath = string.Empty;
            ExportFailed = true;
            ExportError = ex.Message;
        }
    }

    /// <summary>Shows the exported zip in Explorer.</summary>
    [RelayCommand]
    private void RevealExport()
    {
        if (HasExport)
        {
            _logExport?.Reveal(ExportedPath);
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

    partial void OnDeleteImmediatelyChanged(bool value) => _settings.DeleteImmediately = value;

    partial void OnThemeIndexChanged(int value) => _settings.Theme = ThemePalette.FromIndex(value);

    partial void OnPaintStyleIndexChanged(int value) =>
        _settings.PaintStyle = Enum.IsDefined(typeof(PaintStyle), value) ? (PaintStyle)value : PaintStyle.Splashes;

    partial void OnPaintDarkChanged(bool value) => _settings.PaintDark = value;

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
