namespace CleanSlate.Presentation;

/// <summary>
/// <see cref="IProgress{T}"/> that runs its handler on the UI thread when created there, and
/// inline otherwise (tests): unlike <see cref="Progress{T}"/>, it never defers to the thread pool.
/// </summary>
/// <typeparam name="T">Type of the progress values.</typeparam>
public sealed class UiProgress<T> : IProgress<T>
{
    private readonly Action<T> _handler;
    private readonly SynchronizationContext? _context = SynchronizationContext.Current;

    /// <summary>Creates the relay for <paramref name="handler"/>.</summary>
    public UiProgress(Action<T> handler) => _handler = handler;

    /// <inheritdoc/>
    public void Report(T value)
    {
        if (_context is null)
        {
            _handler(value);
        }
        else
        {
            _context.Post(_ => _handler(value), null);
        }
    }
}
