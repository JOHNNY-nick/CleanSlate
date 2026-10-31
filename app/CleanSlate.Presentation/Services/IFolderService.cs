namespace CleanSlate.Presentation.Services;

/// <summary>Folders the user chooses: defaults, checks (<see cref="FolderRules"/>), creation, picker (D63).</summary>
public interface IFolderService
{
    /// <summary>Default data folder: <c>%LOCALAPPDATA%\CleanSlate</c> (quarantine and history).</summary>
    string DefaultQuarantineFolder { get; }

    /// <summary>Default folder of the exported logs: the Desktop.</summary>
    string DefaultExportFolder { get; }

    /// <summary>Checks a quarantine folder on this PC.</summary>
    FolderProblem CheckQuarantineFolder(string path);

    /// <summary>Checks an export folder on this PC.</summary>
    FolderProblem CheckExportFolder(string path);

    /// <summary>
    /// Creates the quarantine folder; a folder other than the default one gets permissions for
    /// SYSTEM and Administrators only, so that a program without administrator rights cannot
    /// alter what is kept there.
    /// </summary>
    void PrepareQuarantineFolder(string path);

    /// <summary>Asks the user for a folder; null when cancelled.</summary>
    Task<string?> PickFolderAsync();
}
