using CleanSlate.Interop;

namespace CleanSlate.Presentation.Services;

/// <summary>Summary shown before any quarantine (EF-16).</summary>
/// <param name="ItemCount">Number of items.</param>
/// <param name="SizeText">Total size, e.g. « 1,2 Go ».</param>
/// <param name="CountByKind">Number of items per kind.</param>
/// <param name="QuarantineDays">Days during which items stay restorable.</param>
/// <param name="Simulation">Simulation mode: nothing will be moved.</param>
/// <param name="RequiresWord">Above 20 GB or 50 000 files: the user must type « QUARANTAINE ».</param>
public sealed record CleanSummary(
    int ItemCount,
    string SizeText,
    IReadOnlyList<KeyValuePair<LeftoverKind, int>> CountByKind,
    int QuarantineDays,
    bool Simulation,
    bool RequiresWord = false);

/// <summary>Answer of the confirmation dialog.</summary>
/// <param name="Confirmed">The user confirmed.</param>
/// <param name="Word">Word typed when it was required.</param>
public sealed record CleanDecision(bool Confirmed, string? Word = null);

/// <summary>Dialogs shown by the ViewModels (implemented with ContentDialog).</summary>
public interface IDialogService
{
    /// <summary>Mandatory summary before quarantine (EF-16, ENF-02).</summary>
    Task<CleanDecision> ConfirmCleanAsync(CleanSummary summary);

    /// <summary>Asks before purging the quarantine for good (EF-19).</summary>
    Task<bool> ConfirmPurgeAsync(int quarantineDays);

    /// <summary>First launch: quarantine delay in days, or null without an answer (D62).</summary>
    Task<int?> ChooseQuarantineDaysAsync(int currentDays);
}
