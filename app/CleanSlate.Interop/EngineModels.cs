using System.Text.Json.Serialization;

namespace CleanSlate.Interop;

/// <summary>
/// Engine settings sent to <c>cs_engine_new</c> (mirrors Rust <c>EngineConfig</c>).
/// </summary>
/// <param name="DryRun">Simulation mode: nothing is modified. True by default.</param>
/// <param name="DataDir">Data folder; null means <c>%LOCALAPPDATA%\CleanSlate</c>.</param>
public sealed record EngineConfig(bool DryRun = true, string? DataDir = null);

/// <summary>Payload of <c>cs_version</c>.</summary>
public sealed record EngineVersion(string Version);

/// <summary>Error part of the response envelope.</summary>
public sealed record EngineError(string Code, string Message, string? Path);

/// <summary>Response envelope <c>{ ok, data | error }</c> returned by every call.</summary>
internal sealed record EngineResponse<T>(bool Ok, T? Data, EngineError? Error);

/// <summary>System.Text.Json source generation, snake_case like the Rust side.</summary>
[JsonSourceGenerationOptions(PropertyNamingPolicy = JsonKnownNamingPolicy.SnakeCaseLower)]
[JsonSerializable(typeof(EngineConfig))]
[JsonSerializable(typeof(EngineResponse<EngineVersion>))]
internal sealed partial class EngineJsonContext : JsonSerializerContext
{
}
