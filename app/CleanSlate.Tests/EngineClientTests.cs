using CleanSlate.Interop;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>Calls the real cleanslate.dll through EngineClient (FFI round trip).</summary>
public sealed class EngineClientTests
{
    [Fact]
    public async Task VersionAsync_ReturnsEngineVersion()
    {
        using var client = new EngineClient();

        string version = await client.VersionAsync(TestContext.Current.CancellationToken);

        Assert.Equal("0.1.0", version);
    }

    [Fact]
    public void GetVersion_ManyCalls_FreesEveryBuffer()
    {
        // Each call allocates a ByteBuffer in Rust and frees it in ReadResponse.
        for (int i = 0; i < 10_000; i++)
        {
            Assert.Equal("0.1.0", EngineClient.GetVersion().Version);
        }
    }

    [Fact]
    public void Constructor_AcceptsExplicitConfig()
    {
        using var client = new EngineClient(new EngineConfig(DryRun: true, DataDir: Path.GetTempPath()));
        Assert.NotNull(client);
    }

    [Fact]
    public async Task VersionAsync_AfterDispose_Throws()
    {
        var client = new EngineClient();
        client.Dispose();
        client.Dispose(); // idempotent

        await Assert.ThrowsAsync<ObjectDisposedException>(() => client.VersionAsync(TestContext.Current.CancellationToken));
    }

    [Fact]
    public void EngineConfig_DefaultsToDryRun()
    {
        Assert.True(new EngineConfig().DryRun);
    }
}
