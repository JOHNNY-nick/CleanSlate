using CleanSlate.Interop;

namespace CleanSlate.Presentation.Services;

/// <summary>
/// The turn of the engine (D82): it runs one long operation at a time and answers <c>BUSY</c>
/// to the others. A request that arrives meanwhile waits here, shown by
/// <see cref="OperationService"/>, and starts as soon as the engine is free.
/// </summary>
public sealed class EngineTurn
{
    private readonly SemaphoreSlim _turn = new(1, 1);
    private readonly OperationService _operations;

    /// <summary>Creates the turn; the waits are shown by <paramref name="operations"/>.</summary>
    public EngineTurn(OperationService operations) => _operations = operations;

    /// <summary>
    /// Runs <paramref name="call"/> when the engine is free. A wait cancelled by the user ends
    /// as <c>CANCELLED</c>, like a cancelled operation.
    /// </summary>
    public async Task<T> RunAsync<T>(Func<Task<T>> call, CancellationToken cancellationToken)
    {
        if (!_turn.Wait(0))
        {
            using IDisposable waiting = _operations.Wait();
            try
            {
                await _turn.WaitAsync(cancellationToken);
            }
            catch (OperationCanceledException)
            {
                throw new EngineException("CANCELLED", "cancelled while waiting for the engine");
            }
        }

        try
        {
            return await call();
        }
        finally
        {
            _turn.Release();
        }
    }
}
