namespace CleanSlate.Interop;

/// <summary>
/// Error reported by the engine (<c>ok: false</c> in the response envelope). Its
/// <see cref="Exception.Message"/> is the French text of the code (<see cref="EngineErrors"/>).
/// </summary>
public sealed class EngineException : Exception
{
    /// <summary>Creates an exception from an engine error code and message.</summary>
    public EngineException(string code, string message, string? path = null)
        : base(EngineErrors.Describe(code, path))
    {
        Code = code;
        Path = path;
        Detail = $"{code}: {message}";
    }

    /// <summary>Stable error code (<c>ACCESS_DENIED</c>, <c>INTERNAL_PANIC</c>…).</summary>
    public string Code { get; }

    /// <summary>Path or registry key concerned, if any.</summary>
    public string? Path { get; }

    /// <summary>Code and message of the engine, in English: for the logs only.</summary>
    public string Detail { get; }
}
