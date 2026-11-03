using CleanSlate.Interop;
using CleanSlate.Presentation;
using CleanSlate.Presentation.Services;

namespace CleanSlate.App.Services;

/// <summary>
/// <see cref="IEngineService"/> backed by <see cref="EngineClient"/>. The engine is created on
/// first use with <c>dry_run = false</c>, given explicitly: the application has no simulation
/// mode (D66), while the engine, the CLI and the tests keep <c>dry_run = true</c> by default
/// (safety rule 5). Only the development mode of a Debug build creates it in simulation (D74).
/// It is recreated when the exclusions or the data folder change.
/// </summary>
public sealed class EngineService : IEngineService, IDisposable
{
    private readonly ISettingsService _settings;
    private readonly IStorageService _storage;
    private readonly object _gate = new();
    private EngineClient? _client;
    private IReadOnlyList<string> _clientExclusions = [];
    private string? _clientDataDir;

    /// <summary>Creates the service.</summary>
    public EngineService(ISettingsService settings, IStorageService storage)
    {
        _settings = settings;
        _storage = storage;
    }

    private EngineClient Client
    {
        get
        {
            lock (_gate)
            {
                IReadOnlyList<string> exclusions = _settings.Exclusions;
                string? dataDir = string.IsNullOrWhiteSpace(_settings.QuarantineFolder) ? null : _settings.QuarantineFolder; // null: %LOCALAPPDATA%\CleanSlate (D63)
                if (_client is null || !_clientExclusions.SequenceEqual(exclusions) || _clientDataDir != dataDir)
                {
                    _client?.Dispose();
                    _client = new EngineClient(new EngineConfig(DryRun: DevMode.Options.EngineDryRun, DataDir: dataDir) { Exclusions = exclusions });
                    _clientExclusions = exclusions.ToList();
                    _clientDataDir = dataDir;
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
    public async Task<Session> CleanAsync(CleanPlan plan, CancellationToken cancellationToken)
    {
        if (plan.DryRun)
        {
            return await Client.CleanAsync(plan, cancellationToken);
        }

        Session session = await _storage.MeasureAsync(
            StorageOperation.Quarantine,
            s => $"{s.Items.Count(i => i.Status is QuarantineStatus.Quarantined or QuarantineStatus.PendingReboot)} élément(s)",
            () => Client.CleanAsync(plan, cancellationToken),
            s => s.FreedBytes,
            s => s.Id);
        _storage.RecordCleaned(session);
        return session;
    }
    /// <inheritdoc/>
    public Task<Session> RestoreAsync(RestoreRequest request, CancellationToken cancellationToken) =>
        Client.RestoreAsync(request, cancellationToken);

    /// <inheritdoc/>
    public Task<IReadOnlyList<Session>> ListSessionsAsync(CancellationToken cancellationToken) =>
        Client.ListSessionsAsync(cancellationToken);

    /// <inheritdoc/>
    public Task<UninstallResult> UninstallAsync(UninstallRequest request, IProgress<UninstallProgress>? progress, CancellationToken cancellationToken) => request.DryRun
        ? Client.UninstallAsync(request, progress, cancellationToken)
        : _storage.MeasureAsync(
            StorageOperation.Uninstall,
            r => r.DisplayName,
            () => Client.UninstallAsync(request, progress, cancellationToken),
            _ => 0UL);

    /// <inheritdoc/>
    public Task<ScanReport> ScanAppLeftoversAsync(AppLeftoversRequest request, IProgress<ScanProgress>? progress, CancellationToken cancellationToken) =>
        Client.ScanAppLeftoversAsync(request, progress, cancellationToken);

    /// <inheritdoc/>
    public async Task<PurgeResult> PurgeAsync(PurgeRequest request, CancellationToken cancellationToken)
    {
        if (request.DryRun)
        {
            return await Client.PurgeAsync(request, cancellationToken);
        }

        PurgeResult result = await _storage.MeasureAsync(
            request.SessionId is null ? StorageOperation.Purge : StorageOperation.DeleteForGood,
            _ => request.SessionId is null ? $"sessions de plus de {request.OlderThanDays} jours" : "session supprimée aussitôt",
            () => Client.PurgeAsync(request, cancellationToken),
            r => r.FreedBytes,
            _ => request.SessionId);
        if (result.FreedBytes > 0)
        {
            _storage.RecordPurged(result.FreedBytes);
        }

        return result;
    }
    /// <inheritdoc/>
    public Task<DiskMap> DiskMapAsync(DiskMapRequest request, IProgress<DiskMapProgress>? progress, CancellationToken cancellationToken) =>
        Client.DiskMapAsync(request, progress, cancellationToken);

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
