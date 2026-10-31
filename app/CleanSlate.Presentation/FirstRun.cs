using CleanSlate.Presentation.Services;

namespace CleanSlate.Presentation;

/// <summary>
/// First launch (D62): the user chooses how long the quarantine keeps what CleanSlate removes
/// (EF-19), before the first automatic purge. The choice can be changed later in Réglages.
/// Without an answer nothing is saved and the question comes back at the next launch.
/// </summary>
public static class FirstRun
{
    /// <summary>Delay proposed by default, in days.</summary>
    public const int DefaultDays = 30;

    /// <summary>Delays offered, in days.</summary>
    public static readonly int[] Choices = [7, 14, 30, 90];

    /// <summary>Asks the delay once, then remembers that it was asked.</summary>
    public static async Task RunAsync(ISettingsService settings, IDialogService dialogs)
    {
        if (settings.FirstRunDone)
        {
            return;
        }

        if (await dialogs.ChooseQuarantineDaysAsync(settings.QuarantineDays) is int days)
        {
            settings.QuarantineDays = Math.Clamp(days, 1, 365);
            settings.FirstRunDone = true;
        }
    }
}
