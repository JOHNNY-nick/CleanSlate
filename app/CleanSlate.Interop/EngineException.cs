namespace CleanSlate.Interop;

/// <summary>
/// Error reported by the engine (<c>ok: false</c> in the response envelope).
/// </summary>
public sealed class EngineException : Exception
{
    /// <summary>Creates an exception from an engine error code and message.</summary>
    public EngineException(string code, string message, string? path = null)
        : base($"{code}: {message}")
    {
        Code = code;
        Path = path;
    }

    /// <summary>Stable error code (<c>ACCESS_DENIED</c>, <c>INTERNAL_PANIC</c>…).</summary>
    public string Code { get; }

    /// <summary>Path or registry key concerned, if any.</summary>
    public string? Path { get; }
}
