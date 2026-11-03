using CleanSlate.Interop;
using CleanSlate.Presentation.ViewModels;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>Badge of a folded group (D72): its best confidence, visible without opening it.</summary>
public sealed class LeftoverGroupBadgeTests
{
    private static Leftover Item(string id, Confidence confidence) =>
        new(id, LeftoverKind.Folder, $@"C:\X\{id}", 1, null, confidence, [], null);

    [Fact]
    public void TheHeaderBadge_ShowsTheBestConfidenceOfTheGroup()
    {
        var group = new LeftoverGroupViewModel(new LeftoverGroup("tool", null, [Item("a", Confidence.Low), Item("b", Confidence.Medium)], 2));
        Assert.Equal(Confidence.Medium, group.TopConfidence);
    }
}

/// <summary>
/// Historique (D76): a purged session showed « Purgée · purge le 30 nov. », a date to come,
/// as if it were still in quarantine. Seen on the owner's PC, where « Supprimer
/// définitivement » was on and every session had been purged at once.
/// </summary>
public sealed class SessionPurgeNoteTests
{
    private static Session With(SessionStatus status) =>
        new(Guid.NewGuid(), new DateTimeOffset(2026, 10, 31, 12, 0, 0, TimeSpan.FromHours(1)), Origin.Orphans, [], 1, [], status, false);

    [Fact]
    public void APurgedSession_ShowsNoFuturePurgeDate()
    {
        var vm = new SessionViewModel(With(SessionStatus.Purged), 30);
        Assert.DoesNotContain("purge le", vm.PurgeNote);
        Assert.Contains("plus restaurable", vm.PurgeNote);
    }

    [Fact]
    public void ASessionInQuarantine_ShowsItsPurgeDate()
    {
        var vm = new SessionViewModel(With(SessionStatus.Completed), 30);
        Assert.Equal("purge le 30 nov. 2026", vm.PurgeNote);
        Assert.Equal("restaurée", new SessionViewModel(With(SessionStatus.Restored), 30).PurgeNote);
    }
}
