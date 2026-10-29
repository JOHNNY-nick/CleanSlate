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

    /// <summary>Releases the native engine.</summary>
    public void Dispose() => _handle.Dispose();

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
