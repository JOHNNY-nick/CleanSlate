using System.Runtime.InteropServices;
using CleanSlate.Presentation.Services;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace CleanSlate.Presentation.ViewModels;

/// <summary>
/// First launch (D63), before the application: (1) welcome with the logo and what CleanSlate is,
/// (2) how it works, in plain words, and acceptance, (3) basic settings: quarantine delay
/// (EF-19), quarantine folder and export folder. Finishing creates the quarantine folder,
/// saves the settings and opens the application. Immediate deletion (D66) is off by default.
/// </summary>
public sealed partial class OnboardingViewModel : ObservableObject
{
    /// <summary>Delays offered, in days.</summary>
    public static readonly int[] DayChoices = [7, 14, 30, 90];

    /// <summary>Delay proposed by default, in days.</summary>
    public const int DefaultDays = 30;

    private readonly ISettingsService _settings;
    private readonly IFolderService _folders;

    /// <summary>Creates the ViewModel with the default choices.</summary>
    public OnboardingViewModel(ISettingsService settings, IFolderService folders)
    {
        _settings = settings;
        _folders = folders;
        _selectedDays = DayChoices.Contains(settings.QuarantineDays) ? settings.QuarantineDays : DefaultDays;
        QuarantineFolder = folders.DefaultQuarantineFolder;
        ExportFolder = folders.DefaultExportFolder;
    }

    /// <summary>Raised when the user has finished: the application can be shown.</summary>
    public event EventHandler? Completed;

    /// <summary>Current step, 1 to 3.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsStep1), nameof(IsStep2), nameof(IsStep3))]
    [NotifyCanExecuteChangedFor(nameof(NextCommand), nameof(BackCommand), nameof(FinishCommand))]
    private int _step = 1;

    /// <summary>« J'ai compris et j'accepte » is checked.</summary>
    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(NextCommand))]
    private bool _accepted;

    /// <summary>Quarantine delay chosen, in days.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SelectedDaysIndex))]
    private int _selectedDays;

    /// <summary>« Supprimer définitivement, sans quarantaine » (D66), off by default.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(KeepsQuarantine))]
    private bool _deleteImmediately;

    /// <summary>Quarantine folder typed or picked.</summary>
    [ObservableProperty]
    private string _quarantineFolder = string.Empty;

    /// <summary>Export folder typed or picked.</summary>
    [ObservableProperty]
    private string _exportFolder = string.Empty;

    /// <summary>Why the quarantine folder is refused.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasQuarantineProblem))]
    [NotifyCanExecuteChangedFor(nameof(FinishCommand))]
    private FolderProblem _quarantineProblem;

    /// <summary>Why the export folder is refused.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasExportProblem))]
    [NotifyCanExecuteChangedFor(nameof(FinishCommand))]
    private FolderProblem _exportProblem;

    /// <summary>The folder window could not open: the path is typed instead.</summary>
    [ObservableProperty]
    private bool _pickerFailed;

    /// <summary>Reason why the quarantine folder could not be created.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasPrepareError))]
    private string _prepareError = string.Empty;

    /// <summary>The quarantine delay applies (immediate deletion not chosen).</summary>
    public bool KeepsQuarantine => !DeleteImmediately;

    /// <summary>Step 1: welcome.</summary>
    public bool IsStep1 => Step == 1;

    /// <summary>Step 2: how it works and acceptance.</summary>
    public bool IsStep2 => Step == 2;

    /// <summary>Step 3: settings.</summary>
    public bool IsStep3 => Step == 3;

    /// <summary>Index of <see cref="SelectedDays"/> in <see cref="DayChoices"/>, for the radio buttons.</summary>
    public int SelectedDaysIndex
    {
        get => Array.IndexOf(DayChoices, SelectedDays);
        set
        {
            if (value >= 0 && value < DayChoices.Length)
            {
                SelectedDays = DayChoices[value];
            }
        }
    }

    /// <summary>The quarantine folder is refused.</summary>
    public bool HasQuarantineProblem => QuarantineProblem != FolderProblem.None;

    /// <summary>The export folder is refused.</summary>
    public bool HasExportProblem => ExportProblem != FolderProblem.None;

    /// <summary>The quarantine folder could not be created.</summary>
    public bool HasPrepareError => PrepareError.Length > 0;

    /// <summary>Next step; the settings only after acceptance.</summary>
    [RelayCommand(CanExecute = nameof(CanGoNext))]
    private void Next() => Step++;

    /// <summary>Previous step.</summary>
    [RelayCommand(CanExecute = nameof(CanGoBack))]
    private void Back() => Step--;

    /// <summary>Picks the quarantine folder.</summary>
    [RelayCommand]
    private async Task PickQuarantineFolderAsync()
    {
        if (await PickAsync() is string path)
        {
            QuarantineFolder = path;
        }
    }

    /// <summary>Picks the export folder.</summary>
    [RelayCommand]
    private async Task PickExportFolderAsync()
    {
        if (await PickAsync() is string path)
        {
            ExportFolder = path;
        }
    }

    /// <summary>Back to <c>%LOCALAPPDATA%\CleanSlate</c>.</summary>
    [RelayCommand]
    private void UseDefaultQuarantineFolder() => QuarantineFolder = _folders.DefaultQuarantineFolder;

    /// <summary>Back to the Desktop.</summary>
    [RelayCommand]
    private void UseDefaultExportFolder() => ExportFolder = _folders.DefaultExportFolder;

    /// <summary>Creates the quarantine folder, saves the settings and opens the application.</summary>
    [RelayCommand(CanExecute = nameof(CanFinish))]
    private void Finish()
    {
        QuarantineProblem = _folders.CheckQuarantineFolder(QuarantineFolder);
        ExportProblem = _folders.CheckExportFolder(ExportFolder);
        if (HasQuarantineProblem || HasExportProblem)
        {
            return;
        }

        string quarantine = FolderRules.Normalize(QuarantineFolder) ?? QuarantineFolder;
        string export = FolderRules.Normalize(ExportFolder) ?? ExportFolder;
        try
        {
            _folders.PrepareQuarantineFolder(quarantine);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            PrepareError = ex.Message;
            return;
        }

        PrepareError = string.Empty;
        _settings.QuarantineDays = SelectedDays;
        _settings.DeleteImmediately = DeleteImmediately;
        _settings.QuarantineFolder = FolderRules.SamePath(quarantine, _folders.DefaultQuarantineFolder) ? null : quarantine;
        _settings.ExportFolder = FolderRules.SamePath(export, _folders.DefaultExportFolder) ? null : export;
        _settings.FirstRunDone = true;
        Completed?.Invoke(this, EventArgs.Empty);
    }

    private bool CanGoNext() => Step == 1 || (Step == 2 && Accepted);

    private bool CanGoBack() => Step > 1;

    private bool CanFinish() => Step == 3 && !HasQuarantineProblem && !HasExportProblem;

    private async Task<string?> PickAsync()
    {
        try
        {
            PickerFailed = false;
            return await _folders.PickFolderAsync();
        }
        catch (Exception ex) when (ex is InvalidOperationException or COMException or UnauthorizedAccessException or NotSupportedException or ArgumentException)
        {
            PickerFailed = true;
            return null;
        }
    }

    partial void OnQuarantineFolderChanged(string value)
    {
        QuarantineProblem = _folders.CheckQuarantineFolder(value);
        PrepareError = string.Empty;
    }

    partial void OnExportFolderChanged(string value) => ExportProblem = _folders.CheckExportFolder(value);
}
