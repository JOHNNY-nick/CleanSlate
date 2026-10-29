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
}
