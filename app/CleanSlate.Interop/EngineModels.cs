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

/// <summary>Kind of leftover item.</summary>
[JsonConverter(typeof(SnakeCaseEnumConverter<LeftoverKind>))]
public enum LeftoverKind
{
    /// <summary>Folder and its content.</summary>
    Folder,

    /// <summary>Single file.</summary>
    File,

    /// <summary>Registry key and its sub-keys.</summary>
    RegistryKey,

    /// <summary>Single registry value.</summary>
    RegistryValue,

    /// <summary>Shortcut whose target is gone.</summary>
    Shortcut,

    /// <summary>Scheduled task whose executable is gone.</summary>
    ScheduledTask,

    /// <summary>Service whose executable is gone.</summary>
    Service,
}

/// <summary>Confidence that an item is a leftover (EF-10).</summary>
[JsonConverter(typeof(SnakeCaseEnumConverter<Confidence>))]
public enum Confidence
{
    /// <summary>Score ≥ 60, pre-checked.</summary>
    High,

    /// <summary>Score 35 to 59.</summary>
    Medium,

    /// <summary>Score &lt; 35.</summary>
    Low,
}

/// <summary>Phase of a scan.</summary>
[JsonConverter(typeof(SnakeCaseEnumConverter<ScanPhase>))]
public enum ScanPhase
{
    /// <summary>Listing candidates.</summary>
    Listing,

    /// <summary>Measuring and scoring.</summary>
    Analyzing,

    /// <summary>Finished or cancelled.</summary>
    Done,
}

/// <summary>One item proposed for cleaning.</summary>
public sealed record Leftover(
    string Id,
    LeftoverKind Kind,
    string Location,
    ulong SizeBytes,
    DateTimeOffset? LastModified,
    Confidence Confidence,
    IReadOnlyList<string> Reasons,
    string? GuessedApp);

/// <summary>Leftovers grouped by guessed application (EF-09).</summary>
public sealed record LeftoverGroup(string Label, string? Publisher, IReadOnlyList<Leftover> Items, ulong TotalBytes);

/// <summary>Error on one item; never stops the operation (ENF-09).</summary>
public sealed record ItemError(string Code, string Message, string? Path);

/// <summary>Result of a scan.</summary>
public sealed record ScanReport(
    IReadOnlyList<LeftoverGroup> Groups,
    IReadOnlyList<ItemError> Errors,
    ulong DurationMs,
    bool Cancelled);

/// <summary>Options of <c>cs_scan_orphans</c>; simulation is always on for a scan.</summary>
public sealed record ScanOptions(bool DryRun = true, bool IncludeRegistry = true, bool IncludeShortcuts = true);

/// <summary>Progress event (at most one per 250 ms).</summary>
public sealed record ScanProgress(ScanPhase Phase, ulong Current, ulong Total, string Path);

/// <summary>System.Text.Json source generation, snake_case like the Rust side.</summary>
[JsonSourceGenerationOptions(PropertyNamingPolicy = JsonKnownNamingPolicy.SnakeCaseLower)]
[JsonSerializable(typeof(EngineConfig))]
[JsonSerializable(typeof(EngineResponse<EngineVersion>))]
[JsonSerializable(typeof(EngineResponse<List<InstalledApp>>))]
[JsonSerializable(typeof(EngineResponse<ScanReport>))]
[JsonSerializable(typeof(ScanOptions))]
[JsonSerializable(typeof(ScanProgress))]
internal sealed partial class EngineJsonContext : JsonSerializerContext
{
}
