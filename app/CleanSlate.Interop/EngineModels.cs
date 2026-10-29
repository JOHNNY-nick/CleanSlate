using System.Text.Json;
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

/// <summary>Enum converter matching serde's <c>rename_all = "snake_case"</c>.</summary>
public sealed class SnakeCaseEnumConverter<TEnum> : JsonStringEnumConverter<TEnum>
    where TEnum : struct, Enum
{
    /// <summary>Creates the converter.</summary>
    public SnakeCaseEnumConverter()
        : base(JsonNamingPolicy.SnakeCaseLower, allowIntegerValues: false)
    {
    }
}

/// <summary>Registry view an application was found in.</summary>
[JsonConverter(typeof(SnakeCaseEnumConverter<RegistryView>))]
public enum RegistryView
{
    /// <summary>HKLM, 64-bit view.</summary>
    Hklm64,

    /// <summary>HKLM, 32-bit view (WOW6432Node).</summary>
    Hklm32,

    /// <summary>HKCU.</summary>
    Hkcu,
}

/// <summary>Kind of installed application.</summary>
[JsonConverter(typeof(SnakeCaseEnumConverter<AppKind>))]
public enum AppKind
{
    /// <summary>Classic Win32 application.</summary>
    Win32,

    /// <summary>Microsoft Store application (read-only, v1.1).</summary>
    Store,
}

/// <summary>Why an application is hidden by default (EF-02).</summary>
[JsonConverter(typeof(SnakeCaseEnumConverter<HiddenReason>))]
public enum HiddenReason
{
    /// <summary><c>SystemComponent = 1</c>.</summary>
    SystemComponent,

    /// <summary>Update, hotfix or service pack.</summary>
    Update,
}

/// <summary>Installer technology.</summary>
[JsonConverter(typeof(SnakeCaseEnumConverter<InstallerKind>))]
public enum InstallerKind
{
    /// <summary>Windows Installer.</summary>
    Msi,

    /// <summary>Inno Setup.</summary>
    InnoSetup,

    /// <summary>NSIS.</summary>
    Nsis,

    /// <summary>Anything else.</summary>
    Unknown,
}

/// <summary>An installed application (mirrors Rust <c>InstalledApp</c>, specs § 5).</summary>
public sealed record InstalledApp(
    string Id,
    string DisplayName,
    string? Publisher,
    string? Version,
    DateOnly? InstallDate,
    string? InstallLocation,
    ulong? EstimatedSizeKb,
    string? UninstallString,
    string? QuietUninstallString,
    bool IsMsi,
    string? ProductCode,
    RegistryView RegistryView,
    AppKind Kind,
    HiddenReason? HiddenReason,
    InstallerKind Installer);

/// <summary>System.Text.Json source generation, snake_case like the Rust side.</summary>
[JsonSourceGenerationOptions(PropertyNamingPolicy = JsonKnownNamingPolicy.SnakeCaseLower)]
[JsonSerializable(typeof(EngineConfig))]
[JsonSerializable(typeof(EngineResponse<EngineVersion>))]
[JsonSerializable(typeof(EngineResponse<List<InstalledApp>>))]
internal sealed partial class EngineJsonContext : JsonSerializerContext
{
}
