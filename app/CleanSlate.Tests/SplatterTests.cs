using CleanSlate.Presentation;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>Paint splashes of the Multicolore theme (D75).</summary>
public sealed class SplatterTests
{
    [Fact]
    public void ASplash_IsAlwaysTheSame_ForTheSameSeed()
    {
        Assert.Equal(Splatter.Generate(3, 100, 100, 80), Splatter.Generate(3, 100, 100, 80));
        Assert.NotEqual(Splatter.Generate(3, 100, 100, 80), Splatter.Generate(4, 100, 100, 80));
    }

    [Fact]
    public void ASplash_HasACoreStreaksAndDroplets_WithinItsReach()
    {
        const double cx = 300, cy = 200, radius = 100;
        IReadOnlyList<SplatterDot> dots = Splatter.Generate(7, cx, cy, radius);

        Assert.InRange(dots.Count, 29, 45);
        Assert.All(dots, d =>
        {
            double dx = d.X + (d.Width / 2) - cx, dy = d.Y + (d.Height / 2) - cy;
            Assert.True(Math.Sqrt((dx * dx) + (dy * dy)) <= Splatter.Reach * radius + 0.001);
            Assert.True(d.Width > 0 && d.Height > 0);
        });
        // Large blots in the middle, small droplets around.
        Assert.Contains(dots, d => d.Width >= radius * 0.45);
        Assert.Contains(dots, d => d.Width <= radius * 0.1);
    }

    [Fact]
    public void NoRadius_NoSplash() => Assert.Empty(Splatter.Generate(1, 0, 0, 0));
}
