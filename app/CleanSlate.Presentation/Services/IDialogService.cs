using CleanSlate.Interop;

namespace CleanSlate.Presentation.Services;

/// <summary>Summary shown before any quarantine (EF-16).</summary>
/// <param name="ItemCount">Number of items.</param>
/// <param name="SizeText">Total size, e.g. « 1,2 Go ».</param>
/// <param name="CountByKind">Number of items per kind.</param>
/// <param name="QuarantineDays">Days during which items stay restorable.</param>
/// <param name="DeleteImmediately">Deleted for good at once, nothing restorable (D66).</param>
/// <param name="RequiresWord">Above 20 GB or 50 000 files: the user must type « QUARANTAINE ».</param>
public sealed record CleanSummary(
    int ItemCount,
    string SizeText,
    IReadOnlyList<KeyValuePair<LeftoverKind, int>> CountByKind,
    int QuarantineDays,
    bool DeleteImmediately,
    bool RequiresWord = false);

/// <summary>Answer of the confirmation dialog.</summary>
/// <param name="Confirmed">The user confirmed.</param>
/// <param name="Word">Word typed when it was required.</param>
public sealed record CleanDecision(bool Confirmed, string? Word = null);

/// <summary>Summary shown before an action of the Gestionnaire (EF-23, EF-35, D79).</summary>
/// <param name="Title">« Supprimer 3 éléments ».</param>
/// <param name="ItemCount">Number of items.</param>
/// <param name="SizeText">Total size.</param>
/// <param name="SensitiveCount">Items in a sensitive zone (expert mode and « CONFIRMER »).</param>
/// <param name="OfferModes">The user chooses « Sûr » or « Immédiat ».</param>
/// <param name="Details">Extra lines: what will happen, the gain freed now.</param>
public sealed record ManagerSummary(string Title, int ItemCount, string SizeText, int SensitiveCount, bool OfferModes, IReadOnlyList<string> Details);

/// <summary>Answer of the confirmation of the Gestionnaire.</summary>
/// <param name="Confirmed">The user confirmed.</param>
/// <param name="Mode">« Sûr » (default) or « Immédiat ».</param>
/// <param name="Word">Word typed, « CONFIRMER », for the sensitive zones.</param>
public sealed record ManagerDecision(bool Confirmed, DeleteMode Mode = DeleteMode.Safe, string? Word = null);

/// <summary>Dialogs shown by the ViewModels (implemented with ContentDialog).</summary>
public interface IDialogService
{
    /// <summary>Mandatory summary before quarantine (EF-16, ENF-02).</summary>
    Task<CleanDecision> ConfirmCleanAsync(CleanSummary summary);

    /// <summary>Asks before purging the quarantine for good (EF-19).</summary>
    Task<bool> ConfirmPurgeAsync(int quarantineDays);

    /// <summary>Confirmation of an action of the Gestionnaire: preview, mode, and « CONFIRMER » when a sensitive zone is concerned (D79).</summary>
    Task<ManagerDecision> ConfirmManagerAsync(ManagerSummary summary);

    /// <summary>Picks a destination folder (move to another drive), or null.</summary>
    Task<string?> PickFolderAsync();
}
