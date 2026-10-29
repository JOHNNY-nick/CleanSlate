using CleanSlate.Interop;
using CleanSlate.Presentation.Services;

namespace CleanSlate.App.Services;

/// <summary>
/// <see cref="IEngineService"/> backed by <see cref="EngineClient"/>. The engine
/// is created on first use, in simulation mode.
/// </summary>
public sealed class EngineService : IEngineService, IDisposable
{
    private readonly Lazy<EngineClient> _client = new(() => new EngineClient(new EngineConfig(DryRun: true)));

    /// <inheritdoc/>
    public Task<string> GetVersionAsync(CancellationToken cancellationToken) =>
        _client.Value.VersionAsync(cancellationToken);

    /// <inheritdoc/>
    public void Dispose()
    {
        if (_client.IsValueCreated)
        {
            _client.Value.Dispose();
        }
    }
}
