using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Text.Json.Serialization.Metadata;
using Microsoft.Win32.SafeHandles;

namespace CleanSlate.Interop;

/// <summary>
/// Safe wrapper around <c>cleanslate.dll</c>. This is the only class that
/// touches native pointers; every other project uses its typed async methods.
/// </summary>
public sealed class EngineClient : IDisposable
{
    private readonly EngineSafeHandle _handle;

    /// <summary>Creates an engine; simulation mode unless <paramref name="config"/> says otherwise.</summary>
    /// <exception cref="EngineException">The engine could not be created.</exception>
    public EngineClient(EngineConfig? config = null)
        : this(JsonSerializer.SerializeToUtf8Bytes(config ?? new EngineConfig(), EngineJsonContext.Default.EngineConfig))
    {
    }

    /// <summary>Creates an engine from raw config JSON (tests send malformed input through it).</summary>
    internal EngineClient(byte[] configJson)
    {
        _handle = EngineSafeHandle.Create(configJson);
    }

    /// <summary>Returns the engine version, e.g. <c>0.1.0</c>.</summary>
    public Task<string> VersionAsync(CancellationToken cancellationToken = default)
    {
        ObjectDisposedException.ThrowIf(_handle.IsClosed, this);
        return Task.Run(() => GetVersion().Version, cancellationToken);
    }

    /// <summary>Lists installed applications, hidden ones included (EF-01, EF-02).</summary>
    public Task<IReadOnlyList<InstalledApp>> ListAppsAsync(CancellationToken cancellationToken = default)
    {
        ObjectDisposedException.ThrowIf(_handle.IsClosed, this);
        return Task.Run<IReadOnlyList<InstalledApp>>(
            () => WithEngine(engine => ListApps(engine)),
            cancellationToken);
    }

    /// <summary>
    /// Scans for orphan leftovers, read-only (EF-05 to EF-11). Cancelling the
    /// token calls <c>cs_cancel</c>: the returned report is then partial and
    /// flagged <see cref="ScanReport.Cancelled"/> (no exception).
    /// </summary>
    public Task<ScanReport> ScanOrphansAsync(
        ScanOptions? options = null,
        IProgress<ScanProgress>? progress = null,
        CancellationToken cancellationToken = default)
    {
        ObjectDisposedException.ThrowIf(_handle.IsClosed, this);
        byte[] json = JsonSerializer.SerializeToUtf8Bytes(options ?? new ScanOptions(), EngineJsonContext.Default.ScanOptions);
        return Task.Run(() => WithEngine(engine => ScanOrphans(engine, json, progress, cancellationToken)));
    }

    /// <summary>Quarantines items of the last scan (EF-17); simulated unless both the engine and the plan say otherwise.</summary>
    public Task<Session> CleanAsync(CleanPlan plan, CancellationToken cancellationToken = default) =>
        CallWithInput(JsonSerializer.SerializeToUtf8Bytes(plan, EngineJsonContext.Default.CleanPlan), CleanNative, EngineJsonContext.Default.EngineResponseSession, cancellationToken);

    /// <summary>Restores a session entirely or partly (EF-18).</summary>
    public Task<Session> RestoreAsync(RestoreRequest request, CancellationToken cancellationToken = default) =>
        CallWithInput(JsonSerializer.SerializeToUtf8Bytes(request, EngineJsonContext.Default.RestoreRequest), RestoreNative, EngineJsonContext.Default.EngineResponseSession, cancellationToken);

    /// <summary>Deletes for good the old sessions (EF-19).</summary>
    public Task<PurgeResult> PurgeAsync(PurgeRequest request, CancellationToken cancellationToken = default) =>
        CallWithInput(JsonSerializer.SerializeToUtf8Bytes(request, EngineJsonContext.Default.PurgeRequest), PurgeNative, EngineJsonContext.Default.EngineResponsePurgeResult, cancellationToken);

    /// <summary>Sessions of the history, most recent first (EF-20).</summary>
    public Task<IReadOnlyList<Session>> ListSessionsAsync(CancellationToken cancellationToken = default)
    {
        ObjectDisposedException.ThrowIf(_handle.IsClosed, this);
        return Task.Run<IReadOnlyList<Session>>(() => WithEngine(ListSessionsNative), cancellationToken);
    }

    /// <summary>
    /// Runs the official uninstaller and waits for its whole process tree
    /// (EF-12 to EF-15). Cancelling only stops waiting (<c>cs_cancel</c>).
    /// </summary>
    public Task<UninstallResult> UninstallAsync(
        UninstallRequest request,
        IProgress<UninstallProgress>? progress = null,
        CancellationToken cancellationToken = default)
    {
        ObjectDisposedException.ThrowIf(_handle.IsClosed, this);
        byte[] json = JsonSerializer.SerializeToUtf8Bytes(request, EngineJsonContext.Default.UninstallRequest);
        return Task.Run(() => WithEngine(engine => Uninstall(engine, json, progress, cancellationToken)));
    }

    /// <summary>Leftovers of an application just uninstalled (EF-13).</summary>
    public Task<ScanReport> ScanAppLeftoversAsync(
        AppLeftoversRequest request,
        IProgress<ScanProgress>? progress = null,
        CancellationToken cancellationToken = default)
    {
        ObjectDisposedException.ThrowIf(_handle.IsClosed, this);
        byte[] json = JsonSerializer.SerializeToUtf8Bytes(request, EngineJsonContext.Default.AppLeftoversRequest);
        return Task.Run(() => WithEngine(engine => ScanOrphans(engine, json, progress, cancellationToken, appLeftovers: true)));
    }

    /// <summary>Releases the native engine.</summary>
    public void Dispose() => _handle.Dispose();

    private static unsafe UninstallResult Uninstall(IntPtr engine, byte[] json, IProgress<UninstallProgress>? progress, CancellationToken cancellationToken)
    {
        GCHandle context = progress is null ? default : GCHandle.Alloc(progress);
        ByteBuffer* buffer;
        try
        {
            using CancellationTokenRegistration registration =
                cancellationToken.Register(() => NativeMethods.cs_cancel((EngineHandle*)engine));
            fixed (byte* input = json)
            {
                delegate* unmanaged[Cdecl]<void*, byte*, int, void> callback = progress is null ? null : &OnUninstallProgress;
                void* ctx = progress is null ? null : (void*)GCHandle.ToIntPtr(context);
                buffer = NativeMethods.cs_uninstall((EngineHandle*)engine, input, json.Length, callback, ctx);
            }
        }
        finally
        {
            // The engine never calls the callback after cs_uninstall returned.
            if (context.IsAllocated)
            {
                context.Free();
            }
        }

        return ReadResponse(buffer, EngineJsonContext.Default.EngineResponseUninstallResult);
    }

    /// <summary>Uninstall progress callback (EF-21), called on the engine's thread.</summary>
    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    private static unsafe void OnUninstallProgress(void* ctx, byte* json, int len)
    {
        // An exception must never cross into native code.
        try
        {
            if (GCHandle.FromIntPtr((IntPtr)ctx).Target is IProgress<UninstallProgress> progress
                && JsonSerializer.Deserialize(new ReadOnlySpan<byte>(json, len), EngineJsonContext.Default.UninstallProgress) is { } value)
            {
                progress.Report(value);
            }
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"CleanSlate uninstall progress dropped: {ex.Message}");
        }
    }

    private Task<T> CallWithInput<T>(
        byte[] json,
        Func<IntPtr, byte[], IntPtr> call,
        JsonTypeInfo<EngineResponse<T>> typeInfo,
        CancellationToken cancellationToken)
    {
        ObjectDisposedException.ThrowIf(_handle.IsClosed, this);
        return Task.Run(() => WithEngine(engine => ReadResponsePointer(call(engine, json), typeInfo)), cancellationToken);
    }

    private static unsafe T ReadResponsePointer<T>(IntPtr buffer, JsonTypeInfo<EngineResponse<T>> typeInfo) =>
        ReadResponse((ByteBuffer*)buffer, typeInfo);

    private static unsafe IntPtr CleanNative(IntPtr engine, byte[] json)
    {
        fixed (byte* input = json)
        {
            return (IntPtr)NativeMethods.cs_clean((EngineHandle*)engine, input, json.Length, null, null);
        }
    }

    private static unsafe IntPtr RestoreNative(IntPtr engine, byte[] json)
    {
        fixed (byte* input = json)
        {
            return (IntPtr)NativeMethods.cs_restore((EngineHandle*)engine, input, json.Length);
        }
    }

    private static unsafe IntPtr PurgeNative(IntPtr engine, byte[] json)
    {
        fixed (byte* input = json)
        {
            return (IntPtr)NativeMethods.cs_purge_quarantine((EngineHandle*)engine, input, json.Length);
        }
    }

    private static unsafe List<Session> ListSessionsNative(IntPtr engine) =>
        ReadResponse(NativeMethods.cs_list_sessions((EngineHandle*)engine), EngineJsonContext.Default.EngineResponseListSession);

    private static unsafe ScanReport ScanOrphans(IntPtr engine, byte[] json, IProgress<ScanProgress>? progress, CancellationToken cancellationToken, bool appLeftovers = false)
    {
        GCHandle context = progress is null ? default : GCHandle.Alloc(progress);
        ByteBuffer* buffer;
        try
        {
            using CancellationTokenRegistration registration =
                cancellationToken.Register(() => NativeMethods.cs_cancel((EngineHandle*)engine));
            fixed (byte* options = json)
            {
                delegate* unmanaged[Cdecl]<void*, byte*, int, void> callback = progress is null ? null : &OnProgress;
                void* ctx = progress is null ? null : (void*)GCHandle.ToIntPtr(context);
                buffer = appLeftovers
                    ? NativeMethods.cs_scan_app_leftovers((EngineHandle*)engine, options, json.Length, callback, ctx)
                    : NativeMethods.cs_scan_orphans((EngineHandle*)engine, options, json.Length, callback, ctx);
            }
        }
        finally
        {
            // The engine never calls the callback after cs_scan_orphans returned.
            if (context.IsAllocated)
            {
                context.Free();
            }
        }

        return ReadResponse(buffer, EngineJsonContext.Default.EngineResponseScanReport);
    }

    /// <summary>Progress callback called by the engine (possibly from a worker thread).</summary>
    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    private static unsafe void OnProgress(void* ctx, byte* json, int len)
    {
        // An exception must never cross into native code.
        try
        {
            if (GCHandle.FromIntPtr((IntPtr)ctx).Target is IProgress<ScanProgress> progress
                && JsonSerializer.Deserialize(new ReadOnlySpan<byte>(json, len), EngineJsonContext.Default.ScanProgress) is { } value)
            {
                progress.Report(value);
            }
        }
        catch (Exception ex)
        {
            // Any exception escaping an UnmanagedCallersOnly method kills the process.
            System.Diagnostics.Debug.WriteLine($"CleanSlate progress dropped: {ex.Message}");
        }
    }

    private static unsafe List<InstalledApp> ListApps(IntPtr engine) =>
        ReadResponse(
            NativeMethods.cs_list_apps((EngineHandle*)engine),
            EngineJsonContext.Default.EngineResponseListInstalledApp);

    /// <summary>Runs <paramref name="call"/> while the native handle cannot be freed.</summary>
    private T WithEngine<T>(Func<IntPtr, T> call)
    {
        bool added = false;
        try
        {
            _handle.DangerousAddRef(ref added);
            return call(_handle.DangerousGetHandle());
        }
        finally
        {
            if (added)
            {
                _handle.DangerousRelease();
            }
        }
    }

    internal static unsafe EngineVersion GetVersion() =>
        ReadResponse(NativeMethods.cs_version(), EngineJsonContext.Default.EngineResponseEngineVersion);

    /// <summary>
    /// Decodes a response buffer, frees it exactly once, and unwraps the envelope.
    /// </summary>
    internal static unsafe T ReadResponse<T>(ByteBuffer* buffer, JsonTypeInfo<EngineResponse<T>> typeInfo)
    {
        if (buffer is null)
        {
            throw new EngineException("NULL_RESPONSE", "The engine returned no response.");
        }

        EngineResponse<T>? response;
        try
        {
            var bytes = new ReadOnlySpan<byte>(buffer->ptr, checked((int)buffer->len));
            response = JsonSerializer.Deserialize(bytes, typeInfo);
        }
        finally
        {
            NativeMethods.cs_buffer_free(buffer);
        }

        return Unwrap(response);
    }

    /// <summary>Turns an envelope into its data, or throws <see cref="EngineException"/>.</summary>
    internal static T Unwrap<T>(EngineResponse<T>? response)
    {
        if (response is null)
        {
            throw new EngineException("INVALID_RESPONSE", "The engine response is empty.");
        }

        if (!response.Ok)
        {
            EngineError error = response.Error ?? new EngineError("UNKNOWN", "Unknown engine error.", null);
            throw new EngineException(error.Code, error.Message, error.Path);
        }

        return response.Data ?? throw new EngineException("INVALID_RESPONSE", "The engine response has no data.");
    }

    /// <summary>Owns the native engine handle and frees it with <c>cs_engine_free</c>.</summary>
    private sealed class EngineSafeHandle : SafeHandleZeroOrMinusOneIsInvalid
    {
        private EngineSafeHandle()
            : base(ownsHandle: true)
        {
        }

        internal static unsafe EngineSafeHandle Create(byte[] configJson)
        {
            EngineHandle* engine;
            ByteBuffer* error = null;
            fixed (byte* config = configJson)
            {
                engine = NativeMethods.cs_engine_new(config, configJson.Length, &error);
            }

            if (engine is null)
            {
                if (error is null)
                {
                    throw new EngineException("ENGINE_CREATE_FAILED", "The CleanSlate engine could not be created.");
                }

                // Frees the buffer and throws EngineException with the engine's error code.
                ReadResponse(error, EngineJsonContext.Default.EngineResponseEngineVersion);
                throw new EngineException("ENGINE_CREATE_FAILED", "The engine reported success without a handle.");
            }

            var safeHandle = new EngineSafeHandle();
            safeHandle.SetHandle((IntPtr)engine);
            return safeHandle;
        }

        protected override unsafe bool ReleaseHandle()
        {
            NativeMethods.cs_engine_free((EngineHandle*)handle);
            return true;
        }
    }
}
