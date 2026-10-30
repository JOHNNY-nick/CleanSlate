using CleanSlate.Interop;

namespace CleanSlate.Presentation.Services;

/// <summary>
/// Facade of the CleanSlate engine for the ViewModels. Every call is async
/// and runs off the UI thread.
/// </summary>
public interface IEngineService
{
    /// <summary>Returns the engine version, e.g. <c>0.1.0</c>.</summary>
    Task<string> GetVersionAsync(CancellationToken cancellationToken);

    /// <summary>Lists installed applications, hidden ones included (EF-01).</summary>
    Task<IReadOnlyList<InstalledApp>> ListAppsAsync(CancellationToken cancellationToken);

    /// <summary>
    /// Scans for orphan leftovers, read-only (EF-05 to EF-11). Cancelling
    /// returns a partial report flagged <see cref="ScanReport.Cancelled"/>.
    /// </summary>
    Task<ScanReport> ScanOrphansAsync(ScanOptions options, IProgress<ScanProgress>? progress, CancellationToken cancellationToken);

    /// <summary>Quarantines items of the last scan (EF-17); simulated if the plan or the engine says so.</summary>
    Task<Session> CleanAsync(CleanPlan plan, CancellationToken cancellationToken);

    /// <summary>Restores a session entirely or partly (EF-18).</summary>
    Task<Session> RestoreAsync(RestoreRequest request, CancellationToken cancellationToken);

    /// <summary>Sessions of the history, most recent first (EF-20).</summary>
    Task<IReadOnlyList<Session>> ListSessionsAsync(CancellationToken cancellationToken);

    /// <summary>Runs the official uninstaller (EF-12 to EF-15); cancelling only stops waiting.</summary>
    Task<UninstallResult> UninstallAsync(UninstallRequest request, IProgress<UninstallProgress>? progress, CancellationToken cancellationToken);

    /// <summary>Leftovers of an application just uninstalled (EF-13).</summary>
    Task<ScanReport> ScanAppLeftoversAsync(AppLeftoversRequest request, IProgress<ScanProgress>? progress, CancellationToken cancellationToken);

    /// <summary>Deletes for good the old sessions (EF-19).</summary>
    Task<PurgeResult> PurgeAsync(PurgeRequest request, CancellationToken cancellationToken);
}
