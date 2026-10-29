using System.Text.Json;
using CleanSlate.Interop;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>Decoding of the { ok, data | error } envelope produced by the Rust side.</summary>
public sealed class EnvelopeTests
{
    private static EngineResponse<EngineVersion>? Parse(string json) =>
        JsonSerializer.Deserialize(json, EngineJsonContext.Default.EngineResponseEngineVersion);

    [Fact]
    public void Unwrap_OkEnvelope_ReturnsData()
    {
        var response = Parse("""{"ok":true,"data":{"version":"9.9.9"}}""");

        Assert.Equal("9.9.9", EngineClient.Unwrap(response).Version);
    }

    [Fact]
    public void Unwrap_ErrorEnvelope_ThrowsEngineException()
    {
        var response = Parse("""{"ok":false,"error":{"code":"ACCESS_DENIED","message":"access denied: C:\\X","path":"C:\\X"}}""");

        var ex = Assert.Throws<EngineException>(() => EngineClient.Unwrap(response));
        Assert.Equal("ACCESS_DENIED", ex.Code);
        Assert.Equal(@"C:\X", ex.Path);
    }

    [Fact]
    public void Unwrap_PanicEnvelope_ThrowsInternalPanic()
    {
        // Shape produced by catch_unwind in cleanslate-ffi (tested on the Rust side).
        var response = Parse("""{"ok":false,"error":{"code":"INTERNAL_PANIC","message":"simulated panic","path":null}}""");

        var ex = Assert.Throws<EngineException>(() => EngineClient.Unwrap(response));
        Assert.Equal("INTERNAL_PANIC", ex.Code);
        Assert.Null(ex.Path);
    }

    [Fact]
    public void Unwrap_MissingData_Throws()
    {
        var ex = Assert.Throws<EngineException>(() => EngineClient.Unwrap(Parse("""{"ok":true}""")));
        Assert.Equal("INVALID_RESPONSE", ex.Code);
    }

    [Fact]
    public void EngineConfig_SerializesInSnakeCase()
    {
        string json = JsonSerializer.Serialize(new EngineConfig(), EngineJsonContext.Default.EngineConfig);

        Assert.Equal("""{"dry_run":true,"data_dir":null,"exclusions":[]}""", json);
    }
}
