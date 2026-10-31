using CleanSlate.Interop;
using CleanSlate.Presentation.Services;

namespace CleanSlate.Presentation;

/// <summary>
/// « Supprimer définitivement, sans quarantaine » (D66). The items always go through the
/// quarantine first (manifest written before the move, safety rules 3, 6 and 7); with this
/// option, the session just created is then purged at once, and only that session.
/// </summary>
public static class ImmediateDeletion
{
    /// <summary>Purges <paramref name="session"/> when the option is on; true when it was.</summary>
    public static async Task<bool> ApplyAsync(IEngineService engine, ISettingsService settings, Session session, CancellationToken cancellationToken)
    {
        if (!settings.DeleteImmediately || session.Simulated
            || !session.Items.Any(i => i.Status is QuarantineStatus.Quarantined or QuarantineStatus.PendingReboot))
        {
            return false;
        }

        await engine.PurgeAsync(new PurgeRequest(0, DryRun: false, SessionId: session.Id), cancellationToken);
        return true;
    }
}
