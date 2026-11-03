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
