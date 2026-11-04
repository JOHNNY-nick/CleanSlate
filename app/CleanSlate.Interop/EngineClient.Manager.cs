using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization.Metadata;

namespace CleanSlate.Interop;

/// <summary>
/// The Gestionnaire through <c>cs_manager</c> (D79): one JSON request by <c>op</c>. Actions
/// are sent with <c>dry_run: false</c> by the application only (D66), after the confirmation;
/// the engine still simulates when it was created in simulation.
/// </summary>
public sealed partial class EngineClient
{
    /// <summary>Largest files, categories, old files, empty folders, candidates for duplicates.</summary>
    public Task<StorageAnalysis> AnalyzeAsync(string root, IProgress<ManagerProgress>? progress = null, CancellationToken cancellationToken = default) =>
        Manager(new JsonObject { ["op"] = "analyze", ["root"] = root }, EngineJsonContext.Default.EngineResponseStorageAnalysis, progress, cancellationToken);

    /// <summary>Duplicates among the candidates of an analysis (SHA-256).</summary>
    public async Task<IReadOnlyList<DuplicateGroup>> DuplicatesAsync(IReadOnlyList<IReadOnlyList<string>> candidates, CancellationToken cancellationToken = default) =>
        await Manager(
            new JsonObject { ["op"] = "duplicates", ["candidates"] = new JsonArray(candidates.Select(g => (JsonNode)Strings(g)).ToArray()) },
            EngineJsonContext.Default.EngineResponseListDuplicateGroup,
            null,
            cancellationToken);

    /// <summary>Regenerable caches.</summary>
    public async Task<IReadOnlyList<CacheItem>> CachesAsync(CancellationToken cancellationToken = default) =>
        await Manager(Op("caches"), EngineJsonContext.Default.EngineResponseListCacheItem, null, cancellationToken);

    /// <summary>Recycle Bins by drive.</summary>
    public async Task<IReadOnlyList<RecycleBinInfo>> RecycleBinsAsync(CancellationToken cancellationToken = default) =>
        await Manager(Op("recycle_bins"), EngineJsonContext.Default.EngineResponseListRecycleBinInfo, null, cancellationToken);

    /// <summary>Drives and partitions.</summary>
    public async Task<IReadOnlyList<VolumeInfo>> VolumesAsync(CancellationToken cancellationToken = default) =>
        await Manager(Op("volumes"), EngineJsonContext.Default.EngineResponseListVolumeInfo, null, cancellationToken);

    /// <summary>Physical disks and their unallocated space.</summary>
    public async Task<IReadOnlyList<DiskInfo>> DisksAsync(CancellationToken cancellationToken = default) =>
        await Manager(Op("disks"), EngineJsonContext.Default.EngineResponseListDiskInfo, null, cancellationToken);

    /// <summary>Deletions waiting for the next restart.</summary>
    public async Task<IReadOnlyList<string>> PendingAtRestartAsync(CancellationToken cancellationToken = default) =>
        await Manager(Op("pending_at_restart"), EngineJsonContext.Default.EngineResponseListString, null, cancellationToken);

    /// <summary>Items of the « Système » tab.</summary>
    public async Task<IReadOnlyList<SystemItem>> SystemItemsAsync(CancellationToken cancellationToken = default) =>
        await Manager(Op("system_items"), EngineJsonContext.Default.EngineResponseListSystemItem, null, cancellationToken);

    /// <summary>Zone of a path.</summary>
    public Task<Zone> ZoneAsync(string path, CancellationToken cancellationToken = default) =>
        Manager(new JsonObject { ["op"] = "zone", ["path"] = path }, EngineJsonContext.Default.EngineResponseZone, null, cancellationToken);

    /// <summary>Deletion in mode « Sûr » or « Immédiat ».</summary>
    public Task<ActionResult> DeleteAsync(IReadOnlyList<string> paths, DeleteMode mode, ManualAction manual, bool dryRun = true, CancellationToken cancellationToken = default) =>
        Action("delete", paths, manual, dryRun, cancellationToken, new JsonObject { ["mode"] = mode == DeleteMode.Immediate ? "immediate" : "safe" });

    /// <summary>Deletion at the next restart.</summary>
    public Task<ActionResult> DeleteAtRestartAsync(IReadOnlyList<string> paths, ManualAction manual, bool dryRun = true, CancellationToken cancellationToken = default) =>
        Action("delete_at_restart", paths, manual, dryRun, cancellationToken);

    /// <summary>Secure deletion (overwritten, then deleted).</summary>
    public Task<ActionResult> SecureDeleteAsync(IReadOnlyList<string> paths, ManualAction manual, bool dryRun = true, CancellationToken cancellationToken = default) =>
        Action("secure_delete", paths, manual, dryRun, cancellationToken);

    /// <summary>Move to another drive, space checked and SHA-256 compared.</summary>
    public Task<ActionResult> MoveAsync(IReadOnlyList<string> paths, string destination, ManualAction manual, bool dryRun = true, CancellationToken cancellationToken = default) =>
        Action("move", paths, manual, dryRun, cancellationToken, new JsonObject { ["destination"] = destination });

    /// <summary>Emptying of caches (never a file in use).</summary>
    public Task<ActionResult> CleanCachesAsync(IReadOnlyList<string> paths, ManualAction manual, bool dryRun = true, CancellationToken cancellationToken = default) =>
        Action("clean_caches", paths, manual, dryRun, cancellationToken);

    /// <summary>Emptying of the Recycle Bin of one drive (« C: ») or of all.</summary>
    public Task<ActionResult> EmptyRecycleBinAsync(string? drive, bool dryRun = true, CancellationToken cancellationToken = default) =>
        Manager(new JsonObject { ["op"] = "empty_recycle_bin", ["drive"] = drive, ["dry_run"] = dryRun }, EngineJsonContext.Default.EngineResponseActionResult, null, cancellationToken);

    /// <summary>A clean-up of Windows.</summary>
    public Task<SystemOutcome> RunSystemAsync(SystemTask task, bool expertMode, bool dryRun = true, CancellationToken cancellationToken = default) =>
        Manager(
            new JsonObject { ["op"] = "system", ["task"] = JsonNamingPolicy.SnakeCaseLower.ConvertName(task.ToString()), ["expert_mode"] = expertMode, ["dry_run"] = dryRun },
            EngineJsonContext.Default.EngineResponseSystemOutcome,
            null,
            cancellationToken);

    /// <summary>Cancels a deletion at the next restart.</summary>
    public async Task CancelAtRestartAsync(string path, bool dryRun = true, CancellationToken cancellationToken = default) =>
        await Manager(new JsonObject { ["op"] = "cancel_at_restart", ["path"] = path, ["dry_run"] = dryRun }, EngineJsonContext.Default.EngineResponseObject, null, cancellationToken, allowNull: true);

    /// <summary>Records a measure of a drive (« Évolution »).</summary>
    public async Task RecordMeasureAsync(DriveMeasure measure, CancellationToken cancellationToken = default)
    {
        JsonObject request = JsonSerializer.SerializeToNode(measure, EngineJsonContext.Default.DriveMeasure)!.AsObject();
        request["op"] = "record_measure";
        await Manager(request, EngineJsonContext.Default.EngineResponseObject, null, cancellationToken, allowNull: true);
    }

    /// <summary>Measures of a drive, oldest first.</summary>
    public async Task<IReadOnlyList<DriveMeasure>> MeasuresAsync(string drive, CancellationToken cancellationToken = default) =>
        await Manager(new JsonObject { ["op"] = "measures", ["drive"] = drive }, EngineJsonContext.Default.EngineResponseListDriveMeasure, null, cancellationToken);

    private static JsonObject Op(string op) => new() { ["op"] = op };

    private static JsonArray Strings(IEnumerable<string> values) => new(values.Select(v => (JsonNode?)JsonValue.Create(v)).ToArray());

    private Task<ActionResult> Action(string op, IReadOnlyList<string> paths, ManualAction manual, bool dryRun, CancellationToken cancellationToken, JsonObject? extra = null)
    {
        var request = new JsonObject
        {
            ["op"] = op,
            ["paths"] = Strings(paths),
            ["manual"] = new JsonObject { ["expert_mode"] = manual.ExpertMode, ["confirmation"] = manual.Confirmation },
            ["dry_run"] = dryRun,
        };
        foreach ((string key, JsonNode? value) in extra?.ToList() ?? [])
        {
            request[key] = value?.DeepClone();
        }

        return Manager(request, EngineJsonContext.Default.EngineResponseActionResult, null, cancellationToken);
    }

    private Task<T> Manager<T>(JsonObject request, JsonTypeInfo<EngineResponse<T>> typeInfo, IProgress<ManagerProgress>? progress, CancellationToken cancellationToken, bool allowNull = false)
    {
        ObjectDisposedException.ThrowIf(_handle.IsClosed, this);
        byte[] json = System.Text.Encoding.UTF8.GetBytes(request.ToJsonString());
        return Task.Run(() => WithEngine(engine => ManagerNative(engine, json, typeInfo, progress, cancellationToken, allowNull)));
    }

    private static unsafe T ManagerNative<T>(IntPtr engine, byte[] json, JsonTypeInfo<EngineResponse<T>> typeInfo, IProgress<ManagerProgress>? progress, CancellationToken cancellationToken, bool allowNull)
    {
        GCHandle context = progress is null ? default : GCHandle.Alloc(progress);
        ByteBuffer* buffer;
        try
        {
            using CancellationTokenRegistration registration =
                cancellationToken.Register(() => NativeMethods.cs_cancel((EngineHandle*)engine));
            fixed (byte* request = json)
            {
                delegate* unmanaged[Cdecl]<void*, byte*, int, void> callback = progress is null ? null : &OnManagerProgress;
                void* ctx = progress is null ? null : (void*)GCHandle.ToIntPtr(context);
                buffer = NativeMethods.cs_manager((EngineHandle*)engine, request, json.Length, callback, ctx);
            }
        }
        finally
        {
            // The engine never calls the callback after cs_manager returned.
            if (context.IsAllocated)
            {
                context.Free();
            }
        }

        return allowNull ? ReadResponseOrDefault(buffer, typeInfo)! : ReadResponse(buffer, typeInfo);
    }

    // An answer without data (record_measure): only the errors matter.
    private static unsafe T? ReadResponseOrDefault<T>(ByteBuffer* buffer, JsonTypeInfo<EngineResponse<T>> typeInfo)
    {
        if (buffer is null)
        {
            throw new EngineException("NULL_RESPONSE", "The engine returned no response.");
        }

        EngineResponse<T>? response;
        try
        {
            response = JsonSerializer.Deserialize(new ReadOnlySpan<byte>(buffer->ptr, checked((int)buffer->len)), typeInfo);
        }
        finally
        {
            NativeMethods.cs_buffer_free(buffer);
        }

        if (response is { Ok: false })
        {
            EngineError error = response.Error ?? new EngineError("UNKNOWN", "Unknown engine error.", null);
            throw new EngineException(error.Code, error.Message, error.Path);
        }

        return response is null ? default : response.Data;
    }

    /// <summary>Progress callback of the Gestionnaire.</summary>
    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    private static unsafe void OnManagerProgress(void* ctx, byte* json, int len)
    {
        // An exception must never cross into native code.
        try
        {
            if (GCHandle.FromIntPtr((IntPtr)ctx).Target is IProgress<ManagerProgress> progress
                && JsonSerializer.Deserialize(new ReadOnlySpan<byte>(json, len), EngineJsonContext.Default.ManagerProgress) is { } value)
            {
                progress.Report(value);
            }
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"CleanSlate manager progress dropped: {ex.Message}");
        }
    }
}
