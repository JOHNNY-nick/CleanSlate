using System.Text.Json;
using CleanSlate.Interop;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>Decoding of cs_scan_orphans output and encoding of its options.</summary>
public sealed class ScanReportJsonTests
{
    private const string Envelope = """
        {"ok":true,"data":{
          "groups":[{"label":"audacity","publisher":null,"total_bytes":4100,"items":[
            {"id":"a1","kind":"folder","location":"C:\\Users\\Ana\\AppData\\Roaming\\audacity","size_bytes":4000,
             "last_modified":"2026-04-12T10:00:00Z","confidence":"high",
             "reasons":["Aucune application installée ne porte un nom ou un éditeur proche"],"guessed_app":"audacity"},
            {"id":"a2","kind":"registry_key","location":"HKCU\\Software\\Audacity","size_bytes":100,
             "last_modified":null,"confidence":"medium","reasons":[],"guessed_app":"Audacity"}]}],
          "errors":[{"code":"ACCESS_DENIED","message":"access denied","path":"C:\\ProgramData\\X"}],
          "duration_ms":7561,"cancelled":true}}
        """;

    [Fact]
    public void ScanEnvelope_IsDecoded()
    {
        var response = JsonSerializer.Deserialize(Envelope, EngineJsonContext.Default.EngineResponseScanReport);
        ScanReport report = EngineClient.Unwrap(response);

        Assert.True(report.Cancelled);
        Assert.Equal(7561UL, report.DurationMs);
        LeftoverGroup group = Assert.Single(report.Groups);
        Assert.Equal(4100UL, group.TotalBytes);
        Assert.Equal(LeftoverKind.Folder, group.Items[0].Kind);
        Assert.Equal(Confidence.High, group.Items[0].Confidence);
        Assert.Equal(new DateTimeOffset(2026, 4, 12, 10, 0, 0, TimeSpan.Zero), group.Items[0].LastModified);
        Assert.Equal(LeftoverKind.RegistryKey, group.Items[1].Kind);
        Assert.Equal(Confidence.Medium, group.Items[1].Confidence);
        Assert.Equal("ACCESS_DENIED", Assert.Single(report.Errors).Code);
    }

    [Fact]
    public void ScanOptions_DefaultToSimulation()
    {
        string json = JsonSerializer.Serialize(new ScanOptions(), EngineJsonContext.Default.ScanOptions);

        Assert.Equal("""{"dry_run":true,"include_registry":true,"include_shortcuts":true,"include_tasks_and_services":true}""", json);
    }

    [Fact]
    public void ScanProgress_IsDecoded()
    {
        var progress = JsonSerializer.Deserialize(
            """{"phase":"analyzing","current":12,"total":400,"path":"C:\\X"}""",
            EngineJsonContext.Default.ScanProgress);

        Assert.Equal(ScanPhase.Analyzing, progress!.Phase);
        Assert.Equal(400UL, progress.Total);
    }
}
