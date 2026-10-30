using CleanSlate.Interop;
using CleanSlate.Presentation.Services;

namespace CleanSlate.App.Services;

/// <summary>
/// <see cref="IEngineService"/> backed by <see cref="EngineClient"/>. The engine
/// is created on first use with <c>dry_run</c> = the simulation setting, and
/// recreated when that setting changes (a new scan is then needed).
/// </summary>
public sealed class EngineService : IEngineService, IDisposable
{
    private readonly ISettingsService _settings;
    private readonly object _gate = new();
    private EngineClient? _client;
    private bool _clientSimulation;
    private IReadOnlyList<string> _clientExclusions = [];

    /// <summary>Creates the service.</summary>
    public EngineService(ISettingsService settings)
    {
        _settings = settings;
    }

    private EngineClient Client
    {
        get
        {
            lock (_gate)
            {
                bool simulation = _settings.SimulationMode;
                IReadOnlyList<string> exclusions = _settings.Exclusions;
                if (_client is null || _clientSimulation != simulation || !_clientExclusions.SequenceEqual(exclusions))
                {
                    _client?.Dispose();
                    _client = new EngineClient(new EngineConfig(DryRun: simulation) { Exclusions = exclusions });
                    _clientSimulation = simulation;
                    _clientExclusions = exclusions.ToList();
                }

                return _client;
            }
        }
    }

    /// <inheritdoc/>
    public Task<string> GetVersionAsync(CancellationToken cancellationToken) =>
        Client.VersionAsync(cancellationToken);

    /// <inheritdoc/>
    public Task<IReadOnlyList<InstalledApp>> ListAppsAsync(CancellationToken cancellationToken) =>
        Client.ListAppsAsync(cancellationToken);

    /// <inheritdoc/>
    public Task<ScanReport> ScanOrphansAsync(ScanOptions options, IProgress<ScanProgress>? progress, CancellationToken cancellationToken) =>
        Client.ScanOrphansAsync(options, progress, cancellationToken);

    /// <inheritdoc/>
    public Task<Session> CleanAsync(CleanPlan plan, CancellationToken cancellationToken) =>
        Client.CleanAsync(plan, cancellationToken);

    /// <inheritdoc/>
    public Task<Session> RestoreAsync(RestoreRequest request, CancellationToken cancellationToken) =>
        Client.RestoreAsync(request, cancellationToken);

    /// <inheritdoc/>
    public Task<IReadOnlyList<Session>> ListSessionsAsync(CancellationToken cancellationToken) =>
        Client.ListSessionsAsync(cancellationToken);

    /// <inheritdoc/>
    public Task<UninstallResult> UninstallAsync(UninstallRequest request, IProgress<UninstallProgress>? progress, CancellationToken cancellationToken) =>
        Client.UninstallAsync(request, progress, cancellationToken);

    /// <inheritdoc/>
    public Task<ScanReport> ScanAppLeftoversAsync(AppLeftoversRequest request, IProgress<ScanProgress>? progress, CancellationToken cancellationToken) =>
        Client.ScanAppLeftoversAsync(request, progress, cancellationToken);

    /// <inheritdoc/>
    public Task<PurgeResult> PurgeAsync(PurgeRequest request, CancellationToken cancellationToken) =>
        Client.PurgeAsync(request, cancellationToken);

    /// <inheritdoc/>
    public void Dispose()
    {
        lock (_gate)
        {
            _client?.Dispose();
            _client = null;
        }
    }
}
