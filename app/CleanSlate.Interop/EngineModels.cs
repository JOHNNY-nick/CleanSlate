using System.Text.Json;
using System.Text.Json.Serialization;

namespace CleanSlate.Interop;

/// <summary>
/// Engine settings sent to <c>cs_engine_new</c> (mirrors Rust <c>EngineConfig</c>).
/// </summary>
/// <param name="DryRun">Simulation mode: nothing is modified. True by default.</param>
/// <param name="DataDir">Data folder; null means <c>%LOCALAPPDATA%\CleanSlate</c>.</param>
public sealed record EngineConfig(bool DryRun = true, string? DataDir = null)
{
    /// <summary>Folders or keys never proposed (EF-20).</summary>
    public IReadOnlyList<string> Exclusions { get; init; } = [];
}

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

    /// <summary>Not in the registry: a Store package of the current user (EF-04).</summary>
    Package,
}

/// <summary>Kind of installed application.</summary>
[JsonConverter(typeof(SnakeCaseEnumConverter<AppKind>))]
public enum AppKind
{
    /// <summary>Classic Win32 application.</summary>
    Win32,

    /// <summary>Microsoft Store application (MSIX package of the current user, EF-04).</summary>
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

    /// <summary>Store application published by Microsoft: never proposed for removal (D52).</summary>
    MicrosoftApp,

    /// <summary>Package preinstalled for every user by Windows or the PC maker: never proposed for removal (D52).</summary>
    Preinstalled,
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

    /// <summary>MSIX / AppX package (Microsoft Store).</summary>
    Msix,

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
    InstallerKind Installer,
    string? PackageFullName = null,
    string? PackageFamilyName = null);

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
public sealed record ScanOptions(bool DryRun = true, bool IncludeRegistry = true, bool IncludeShortcuts = true, bool IncludeTasksAndServices = true);

/// <summary>Progress event (at most one per 250 ms).</summary>
public sealed record ScanProgress(ScanPhase Phase, ulong Current, ulong Total, string Path);

/// <summary>What started a session: <c>{"kind":"orphans"}</c> or <c>{"kind":"app","app_id":"…"}</c>.</summary>
public sealed record Origin(
    string Kind,
    [property: JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? AppId = null)
{
    /// <summary>Scan of orphan leftovers.</summary>
    public static Origin Orphans { get; } = new("orphans");

    /// <summary>Full uninstall of one application.</summary>
    public static Origin ForApp(string appId) => new("app", appId);
}

/// <summary>Items the user chose to quarantine (EF-16); simulation by default.</summary>
public sealed record CleanPlan(
    IReadOnlyList<string> ItemIds,
    Origin Origin,
    bool DryRun = true,
    [property: JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? Confirmation = null);

/// <summary>State of one quarantined item.</summary>
[JsonConverter(typeof(SnakeCaseEnumConverter<QuarantineStatus>))]
public enum QuarantineStatus
{
    /// <summary>In quarantine, restorable.</summary>
    Quarantined,

    /// <summary>Put back.</summary>
    Restored,

    /// <summary>Deleted for good.</summary>
    Purged,

    /// <summary>Locked: moved at the next reboot.</summary>
    PendingReboot,

    /// <summary>Not quarantined; see the error.</summary>
    Failed,
}

/// <summary>State of a session.</summary>
[JsonConverter(typeof(SnakeCaseEnumConverter<SessionStatus>))]
public enum SessionStatus
{
    /// <summary>Every item quarantined.</summary>
    Completed,

    /// <summary>Some items failed.</summary>
    Partial,

    /// <summary>Restored.</summary>
    Restored,

    /// <summary>Purged.</summary>
    Purged,
}

/// <summary>An item stored in quarantine (specs § 5).</summary>
public sealed record QuarantinedItem(
    string Id,
    LeftoverKind Kind,
    string OriginalLocation,
    string QuarantinePath,
    ulong SizeBytes,
    string? Sha256,
    string? OriginalAcl,
    DateTimeOffset QuarantinedAt,
    DateTimeOffset? RestoredAt,
    QuarantineStatus Status,
    ItemError? Error);

/// <summary>A cleaning session (EF-20); <see cref="Simulated"/> when nothing was touched.</summary>
public sealed record Session(
    Guid Id,
    DateTimeOffset CreatedAt,
    Origin Origin,
    IReadOnlyList<QuarantinedItem> Items,
    ulong FreedBytes,
    IReadOnlyList<ItemError> Errors,
    SessionStatus Status,
    bool Simulated = false);

/// <summary>Request of <c>cs_restore</c>; simulation by default.</summary>
public sealed record RestoreRequest(
    Guid SessionId,
    [property: JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] IReadOnlyList<string>? ItemIds = null,
    bool DryRun = true);

/// <summary>Request of <c>cs_purge_quarantine</c>; simulation by default.</summary>
public sealed record PurgeRequest(uint OlderThanDays, bool DryRun = true);

/// <summary>Result of <c>cs_purge_quarantine</c>.</summary>
public sealed record PurgeResult(ulong FreedBytes, bool Simulated);

/// <summary>Step of an uninstall (EF-21).</summary>
[JsonConverter(typeof(SnakeCaseEnumConverter<UninstallStep>))]
public enum UninstallStep
{
    /// <summary>Creating the restore point.</summary>
    RestorePoint,

    /// <summary>Starting the official uninstaller.</summary>
    Launching,

    /// <summary>Waiting for the uninstaller and its process tree.</summary>
    Waiting,

    /// <summary>Checking that the application is gone.</summary>
    Verifying,

    /// <summary>Finished.</summary>
    Done,
}

/// <summary>One process of the uninstaller's Job Object.</summary>
public sealed record JobProcess(uint Pid, string Name);

/// <summary>
/// Progress event of <c>cs_uninstall</c> (EF-21): each step, then at most every 250 ms.
/// <see cref="Percent"/> and <see cref="Action"/> come from Windows Installer or the Store;
/// for an <c>.exe</c> uninstaller they stay empty and <see cref="Processes"/> lists the Job Object.
/// </summary>
public sealed record UninstallProgress(
    UninstallStep Step,
    ulong ElapsedMs,
    double? Percent,
    string? Action,
    IReadOnlyList<JobProcess> Processes);

/// <summary>Request of <c>cs_uninstall</c>; simulation by default, restore point requested.</summary>
public sealed record UninstallRequest(string AppId, bool Quiet = false, bool CreateRestorePoint = true, bool DryRun = true);

/// <summary>Result of <c>cs_uninstall</c>.</summary>
public sealed record UninstallResult(
    string AppId,
    string DisplayName,
    string Command,
    bool Simulated,
    bool RestorePointCreated,
    string? RestorePointError,
    uint? ExitCode,
    bool TimedOut,
    bool Cancelled,
    bool StillInstalled);

/// <summary>
/// Request of <c>cs_scan_app_leftovers</c>. <see cref="UninstallerMissing"/>: the application
/// is still listed but its uninstaller is missing; leftovers are searched anyway, at most
/// Medium (design § 5, D45).
/// </summary>
public sealed record AppLeftoversRequest(string AppId, bool UninstallerMissing = false);

/// <summary>System.Text.Json source generation, snake_case like the Rust side.</summary>
[JsonSourceGenerationOptions(PropertyNamingPolicy = JsonKnownNamingPolicy.SnakeCaseLower)]
[JsonSerializable(typeof(EngineConfig))]
[JsonSerializable(typeof(EngineResponse<EngineVersion>))]
[JsonSerializable(typeof(EngineResponse<List<InstalledApp>>))]
[JsonSerializable(typeof(EngineResponse<ScanReport>))]
[JsonSerializable(typeof(ScanOptions))]
[JsonSerializable(typeof(ScanProgress))]
[JsonSerializable(typeof(CleanPlan))]
[JsonSerializable(typeof(RestoreRequest))]
[JsonSerializable(typeof(PurgeRequest))]
[JsonSerializable(typeof(EngineResponse<Session>))]
[JsonSerializable(typeof(EngineResponse<List<Session>>))]
[JsonSerializable(typeof(EngineResponse<PurgeResult>))]
[JsonSerializable(typeof(UninstallRequest))]
[JsonSerializable(typeof(UninstallProgress))]
[JsonSerializable(typeof(AppLeftoversRequest))]
[JsonSerializable(typeof(EngineResponse<UninstallResult>))]
internal sealed partial class EngineJsonContext : JsonSerializerContext
{
}
