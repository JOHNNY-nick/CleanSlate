using CleanSlate.Presentation;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>Paint of the Multicolore theme (D76): organic, colorful, different on every page.</summary>
public sealed class PaintComposerTests
{
    private static readonly string[] Pages = ["HomePage", "AppsPage", "StoragePage", "OrphansPage", "HistoryPage", "SettingsPage"];

    [Theory]
    [InlineData(PaintStyle.Splashes)]
    [InlineData(PaintStyle.Watercolor)]
    [InlineData(PaintStyle.Explosion)]
    public void EachStyle_IsTheSameEveryTime_ButDifferentOnEveryPage(PaintStyle style)
    {
        Assert.Equal(Points(PaintComposer.Compose(style, "HomePage")), Points(PaintComposer.Compose(style, "HomePage")));
        var compositions = Pages.Select(p => string.Join(";", Points(PaintComposer.Compose(style, p)).Take(40))).ToList();
        Assert.Equal(Pages.Length, compositions.Distinct().Count());
    }

    [Fact]
    public void TheThreeStyles_AreDifferent()
    {
        var counts = Enum.GetValues<PaintStyle>().Select(s => PaintComposer.Compose(s, "HomePage")).ToList();
        Assert.Equal(3, counts.Select(c => string.Join(";", Points(c).Take(20))).Distinct().Count());
        // Watercolor leans to translucent stains that blend; the others to vivid paint.
        static int Stains(PaintStyle style) => PaintComposer.Compose(style, "HomePage").Count(s => s.Opacity < 0.5);
        Assert.True(Stains(PaintStyle.Watercolor) > 2 * Stains(PaintStyle.Splashes));
    }

    [Theory]
    [InlineData(PaintStyle.Splashes)]
    [InlineData(PaintStyle.Watercolor)]
    [InlineData(PaintStyle.Explosion)]
    public void Shapes_AreCurves_InEveryColor_AndRunOffTheEdges(PaintStyle style)
    {
        IReadOnlyList<PaintShape> shapes = PaintComposer.Compose(style, "AppsPage");
        Assert.InRange(shapes.Count, 600, 4000); // grouped by color when drawn: stays light
        Assert.All(shapes, s =>
        {
            Assert.True(s.Segments.Count >= 24 && s.Segments.Count % 3 == 0, "a smooth closed curve, never a box or a line");
            Assert.InRange(s.Color, 0, PaintComposer.ColorCount - 1);
            Assert.InRange(s.Opacity, 0.05, 1);
        });
        Assert.True(shapes.Select(s => s.Color).Distinct().Count() >= 7, "every color");
        IEnumerable<PaintPoint> all = shapes.SelectMany(s => s.Segments);
        Assert.True(all.Any(p => p.X < 0 || p.X > PaintComposer.Width || p.Y < 0 || p.Y > PaintComposer.Height), "paint runs off the edges");
    }

    /// <summary>
    /// The owner wants « at least three times more color »: very large splats (a third of the
    /// area), drips, streaks, hundreds of drops and grain, blended watercolor and gradients, in
    /// every style; each page leans to its own color.
    /// </summary>
    [Theory]
    [InlineData(PaintStyle.Splashes)]
    [InlineData(PaintStyle.Watercolor)]
    [InlineData(PaintStyle.Explosion)]
    public void TheWall_IsRichAndVaried(PaintStyle style)
    {
        IReadOnlyList<PaintShape> shapes = PaintComposer.Compose(style, "HistoryPage", dominant: 5);
        static (double W, double H) Size(PaintShape s) =>
            (s.Segments.Max(p => p.X) - s.Segments.Min(p => p.X), s.Segments.Max(p => p.Y) - s.Segments.Min(p => p.Y));

        Assert.Contains(shapes, s => Size(s).W >= PaintComposer.Width / 4.5); // very large splats
        Assert.True(shapes.Count(s => Size(s).W < 14) >= 600, "hundreds of drops and grain");
        Assert.Contains(shapes, s => Size(s).H > Size(s).W * 3 && Size(s).H > 40); // drips and buildings
        Assert.Contains(shapes, s => s.Opacity < 0.5); // watercolor that blends
        Assert.Contains(shapes, s => s.Color2 >= 0);  // gradients
        double share = shapes.Count(s => s.Color == 5) / (double)shapes.Count;
        Assert.True(share > 0.25, $"dominant color {share:P0}");
    }

    private static IEnumerable<string> Points(IReadOnlyList<PaintShape> shapes) =>
        shapes.SelectMany(s => s.Segments).Select(p => $"{p.X:0.#},{p.Y:0.#}");
}
