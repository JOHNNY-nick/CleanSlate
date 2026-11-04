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
public sealed class EngineService : IEngineService, IManagerService, IDisposable
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
    public async Task<Session> CleanAsync(CleanPlan plan, IProgress<CleanProgress>? progress, CancellationToken cancellationToken)
    {
        if (plan.DryRun)
        {
            return await Client.CleanAsync(plan, progress, cancellationToken);
        }

        Session session = await _storage.MeasureAsync(
            StorageOperation.Quarantine,
            s => $"{s.Items.Count(i => i.Status is QuarantineStatus.Quarantined or QuarantineStatus.PendingReboot)} élément(s)",
            () => Client.CleanAsync(plan, progress, cancellationToken),
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

    // Gestionnaire (D79): analyses read-only; actions real (D66), after the confirmation.

    /// <inheritdoc/>
    public Task<StorageAnalysis> AnalyzeAsync(string root, IProgress<ManagerProgress>? progress, CancellationToken cancellationToken) =>
        Client.AnalyzeAsync(root, progress, cancellationToken);

    /// <inheritdoc/>
    public Task<IReadOnlyList<DuplicateGroup>> DuplicatesAsync(IReadOnlyList<IReadOnlyList<string>> candidates, CancellationToken cancellationToken) =>
        Client.DuplicatesAsync(candidates, cancellationToken);

    /// <inheritdoc/>
    public Task<IReadOnlyList<CacheItem>> CachesAsync(CancellationToken cancellationToken) => Client.CachesAsync(cancellationToken);

    /// <inheritdoc/>
    public Task<IReadOnlyList<RecycleBinInfo>> RecycleBinsAsync(CancellationToken cancellationToken) => Client.RecycleBinsAsync(cancellationToken);

    /// <inheritdoc/>
    public Task<IReadOnlyList<VolumeInfo>> VolumesAsync(CancellationToken cancellationToken) => Client.VolumesAsync(cancellationToken);

    /// <inheritdoc/>
    public Task<IReadOnlyList<string>> PendingAtRestartAsync(CancellationToken cancellationToken) => Client.PendingAtRestartAsync(cancellationToken);

    /// <inheritdoc/>
    public Task<IReadOnlyList<SystemItem>> SystemItemsAsync(CancellationToken cancellationToken) => Client.SystemItemsAsync(cancellationToken);

    /// <inheritdoc/>
    public Task<ActionResult> DeleteAsync(IReadOnlyList<string> paths, DeleteMode mode, ManualAction manual, CancellationToken cancellationToken) =>
        Client.DeleteAsync(paths, mode, manual, dryRun: false, cancellationToken);

    /// <inheritdoc/>
    public Task<ActionResult> DeleteAtRestartAsync(IReadOnlyList<string> paths, ManualAction manual, CancellationToken cancellationToken) =>
        Client.DeleteAtRestartAsync(paths, manual, dryRun: false, cancellationToken);

    /// <inheritdoc/>
    public Task<ActionResult> SecureDeleteAsync(IReadOnlyList<string> paths, ManualAction manual, CancellationToken cancellationToken) =>
        Client.SecureDeleteAsync(paths, manual, dryRun: false, cancellationToken);

    /// <inheritdoc/>
    public Task<ActionResult> MoveAsync(IReadOnlyList<string> paths, string destination, ManualAction manual, CancellationToken cancellationToken) =>
        Client.MoveAsync(paths, destination, manual, dryRun: false, cancellationToken);

    /// <inheritdoc/>
    public Task<ActionResult> CleanCachesAsync(IReadOnlyList<string> paths, ManualAction manual, CancellationToken cancellationToken) =>
        Client.CleanCachesAsync(paths, manual, dryRun: false, cancellationToken);

    /// <inheritdoc/>
    public Task<ActionResult> EmptyRecycleBinAsync(string? drive, CancellationToken cancellationToken) =>
        Client.EmptyRecycleBinAsync(drive, dryRun: false, cancellationToken);

    /// <inheritdoc/>
    public Task<SystemOutcome> RunSystemAsync(SystemTask task, bool expertMode, CancellationToken cancellationToken) =>
        Client.RunSystemAsync(task, expertMode, dryRun: false, cancellationToken);

    /// <inheritdoc/>
    public Task RecordMeasureAsync(DriveMeasure measure, CancellationToken cancellationToken) => Client.RecordMeasureAsync(measure, cancellationToken);

    /// <inheritdoc/>
    public Task<IReadOnlyList<DriveMeasure>> MeasuresAsync(string drive, CancellationToken cancellationToken) => Client.MeasuresAsync(drive, cancellationToken);

    /// <inheritdoc/>
    public string SaveReport(string fileName, string content)
    {
        string folder = string.IsNullOrWhiteSpace(_settings.ExportFolder)
            ? Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory)
            : _settings.ExportFolder;
        string path = Path.Combine(folder, fileName);
        File.WriteAllText(path, content, new System.Text.UTF8Encoding(encoderShouldEmitUTF8Identifier: true));
        return path;
    }

    /// <inheritdoc/>
    public void Reveal(string path) =>
        System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Windows), "explorer.exe"))
        {
            ArgumentList = { "/select,", path },
        });

    /// <inheritdoc/>
    public void CopyPath(string path)
    {
        var package = new Windows.ApplicationModel.DataTransfer.DataPackage();
        package.SetText(path);
        Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
    }

    /// <inheritdoc/>
    public void ShowProperties(string path) =>
        System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(path) { UseShellExecute = true, Verb = "properties" });

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
