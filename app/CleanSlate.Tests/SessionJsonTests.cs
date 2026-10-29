using System.Text.Json;
using CleanSlate.Interop;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>JSON contract of the J4 calls (cs_clean, cs_restore, cs_purge_quarantine).</summary>
public sealed class SessionJsonTests
{
    [Fact]
    public void CleanPlan_IsSimulatedByDefault_AndTagsOrigin()
    {
        string json = JsonSerializer.Serialize(new CleanPlan(["a1"], Origin.Orphans), EngineJsonContext.Default.CleanPlan);
        Assert.Equal("""{"item_ids":["a1"],"origin":{"kind":"orphans"},"dry_run":true}""", json);

        string app = JsonSerializer.Serialize(new CleanPlan(["a1"], Origin.ForApp("x"), DryRun: false, Confirmation: "QUARANTAINE"), EngineJsonContext.Default.CleanPlan);
        Assert.Contains("\"origin\":{\"kind\":\"app\",\"app_id\":\"x\"}", app);
        Assert.Contains("\"dry_run\":false", app);
        Assert.Contains("\"confirmation\":\"QUARANTAINE\"", app);
    }

    [Fact]
    public void RestoreAndPurgeRequests_AreSimulatedByDefault()
    {
        var id = Guid.Parse("11111111-2222-3333-4444-555555555555");
        Assert.Equal(
            """{"session_id":"11111111-2222-3333-4444-555555555555","dry_run":true}""",
            JsonSerializer.Serialize(new RestoreRequest(id), EngineJsonContext.Default.RestoreRequest));
        Assert.Equal(
            """{"older_than_days":30,"dry_run":true}""",
            JsonSerializer.Serialize(new PurgeRequest(30), EngineJsonContext.Default.PurgeRequest));
    }

    [Fact]
    public void SessionEnvelope_IsDecoded()
    {
        const string envelope = """
            {"ok":true,"data":{"id":"11111111-2222-3333-4444-555555555555","created_at":"2026-10-29T12:00:00Z",
             "origin":{"kind":"orphans"},"freed_bytes":4000,"status":"partial","simulated":false,
             "errors":[{"code":"LOCKED","message":"utilisé","path":"C:\\X"}],
             "items":[{"id":"i1","kind":"folder","original_location":"C:\\A","quarantine_path":"C:\\Q\\i1\\A",
               "size_bytes":4000,"sha256":"ab","original_acl":"O:BA","quarantined_at":"2026-10-29T12:00:01Z",
               "restored_at":null,"status":"pending_reboot","error":null}]}}
            """;
        Session session = EngineClient.Unwrap(JsonSerializer.Deserialize(envelope, EngineJsonContext.Default.EngineResponseSession));

        Assert.Equal(SessionStatus.Partial, session.Status);
        Assert.Equal("orphans", session.Origin.Kind);
        Assert.Equal(QuarantineStatus.PendingReboot, session.Items[0].Status);
        Assert.Equal("LOCKED", session.Errors[0].Code);
        Assert.False(session.Simulated);
    }
}
