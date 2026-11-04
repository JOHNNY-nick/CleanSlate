namespace CleanSlate.Presentation;

/// <summary>Style of the paint of the Multicolore theme (D76), chosen in Réglages.</summary>
public enum PaintStyle
{
    /// <summary>« Éclaboussures »: vivid splats, droplets, spikes and drips on a plain background.</summary>
    Splashes,

    /// <summary>« Aquarelle »: translucent stains with ragged edges that blend where they overlap.</summary>
    Watercolor,

    /// <summary>« Explosion »: streaks thrown out of a center, droplets along the rays.</summary>
    Explosion,
}

/// <summary>A point of a paint shape.</summary>
/// <param name="X">Horizontal position.</param>
/// <param name="Y">Vertical position.</param>
public readonly record struct PaintPoint(double X, double Y);

/// <summary>
/// One closed shape of paint: from <paramref name="Start"/>, cubic Bézier segments given three
/// points at a time (two control points, then the end point). Never a straight line.
/// </summary>
/// <param name="Start">First point.</param>
/// <param name="Segments">Control, control, end; a multiple of three points.</param>
/// <param name="Color">Index in the paint colors (<c>Paint0</c> to <c>Paint7</c>).</param>
/// <param name="Opacity">0 to 1; the watercolor layers stay translucent so that colors mix.</param>
/// <param name="Color2">Second color of a gradient across the shape, or -1 for a plain one.</param>
public sealed record PaintShape(PaintPoint Start, IReadOnlyList<PaintPoint> Segments, int Color, double Opacity, int Color2 = -1);

/// <summary>
/// Paint of the Multicolore theme (D76), drawn by CleanSlate itself (the reference pictures of
/// the owner only inspire it and are never shipped): organic shapes only, smooth closed curves
/// through jittered points; every color, overlapping; a free, asymmetric composition that runs
/// off the edges, different for every page. Pure and seeded: computed once per page and style,
/// in a virtual area of <see cref="Width"/> × <see cref="Height"/> that the view stretches.
/// </summary>
public static class PaintComposer
{
    /// <summary>Width of the virtual area of a page composition.</summary>
    public const double Width = 1600;

    /// <summary>Height of the virtual area of a page composition.</summary>
    public const double Height = 1100;

    /// <summary>Paint colors: red, orange, yellow, green, blue, violet, pink, turquoise.</summary>
    public const int ColorCount = 8;

    /// <summary>
    /// The paint wall of <paramref name="page"/> in <paramref name="style"/> (D76, street style):
    /// very large splats (some a third of the area), medium ones, watercolor stains that bleed
    /// and blend, streaks out of a burst, drips, hundreds of drops, spray grain and a city
    /// skyline. The style changes the mix; <paramref name="dominant"/> (a color index, or -1)
    /// is the color the page leans to.
    /// </summary>
    public static IReadOnlyList<PaintShape> Compose(PaintStyle style, string page, int dominant = -1)
    {
        var random = new Random(Seed(page) ^ ((int)style * 7919));
        var shapes = new List<PaintShape>();
        int Pick() => dominant >= 0 && random.NextDouble() < 0.35 ? dominant : random.Next(ColorCount);
        (int stains, int big, int medium, int bursts) = style switch
        {
            PaintStyle.Watercolor => (16, 2, 5, 0),
            PaintStyle.Explosion => (5, 2, 5, 2),
            _ => (6, 4, 9, 1),
        };

        // Watercolor first, at the back: translucent layers that blend where they overlap.
        for (int i = 0; i < stains; i++)
        {
            double cx = (random.NextDouble() * (Width + 300)) - 150, cy = (random.NextDouble() * (Height + 200)) - 100;
            double r = 120 + (random.NextDouble() * 220);
            int color = Pick();
            for (int layer = 0; layer < 3; layer++)
            {
                double dx = (random.NextDouble() - 0.5) * r * 0.35, dy = (random.NextDouble() - 0.5) * r * 0.35;
                shapes.Add(Blob(random, cx + dx, cy + dy, r * (1 - (layer * 0.2)), 0.38, 22, layer == 1 ? Pick() : color, 0.28));
            }
        }

        // Bursts: streaks thrown out of a center, with drops at their tips.
        for (int b = 0; b < bursts; b++)
        {
            double cx = 200 + (random.NextDouble() * 1200), cy = 100 + (random.NextDouble() * 900);
            Burst(random, shapes, cx, cy, b == 0 ? 60 : 36, b == 0 ? 950 : 600, Pick);
        }

        // Very large splats on a diagonal (a third of the area), then medium ones anywhere.
        (PaintPoint from, PaintPoint to) = Diagonal(random);
        for (int i = 0; i < big; i++)
        {
            double t = (i + (0.5 * random.NextDouble())) / big;
            Splat(random, shapes, Lerp(from.X, to.X, t) + ((random.NextDouble() - 0.5) * 400), Lerp(from.Y, to.Y, t) + ((random.NextDouble() - 0.5) * 300), 170 + (random.NextDouble() * 120), Pick(), Pick());
        }

        for (int i = 0; i < medium; i++)
        {
            Splat(random, shapes, (random.NextDouble() * (Width + 100)) - 50, (random.NextDouble() * (Height + 100)) - 50, 45 + (random.NextDouble() * 85), Pick(), -1);
        }

        Skyline(random, shapes, random.NextDouble() < 0.5);
        Scatter(random, shapes, 90, 6, 22, 0.95, Pick);  // drops of every size
        Scatter(random, shapes, 320, 1.5, 6, 0.9, Pick); // fine spatter
        Scatter(random, shapes, 600, 0.6, 1.6, 0.55, Pick); // spray grain
        return shapes;
    }

    // A city skyline sprayed along the bottom edge, its tops a little rough.
    private static void Skyline(Random random, List<PaintShape> shapes, bool left)
    {
        double x = left ? -20 : Width - 360;
        double end = x + 380;
        while (x < end)
        {
            double w = 34 + (random.NextDouble() * 46), h = 120 + (random.NextDouble() * 260);
            double top = Height - h, bottom = Height + 30;
            var points = new List<PaintPoint>();
            for (double t = 0; t <= 1.0001; t += 0.25)
            {
                points.Add(new PaintPoint(x + (w * t), top + ((random.NextDouble() - 0.5) * 3))); // roof
            }

            if (random.NextDouble() < 0.4)
            {
                points.Insert(2, new PaintPoint(x + (w / 2), top - 30 - (random.NextDouble() * 40))); // spire
            }

            for (double y = top + 30; y < bottom; y += 40)
            {
                points.Add(new PaintPoint(x + w + ((random.NextDouble() - 0.5) * 2), y));
            }

            for (double y = bottom; y > top + 30; y -= 40)
            {
                points.Add(new PaintPoint(x + ((random.NextDouble() - 0.5) * 2), y));
            }

            shapes.Add(Smooth(points, 4, 0.85, 5));
            x += w + 4 + (random.NextDouble() * 10);
        }
    }

    // Streaks thrown out of a center, wide at the start, pointed at the tip.
    private static void Burst(Random random, List<PaintShape> shapes, double cx, double cy, int streaks, double reach, Func<int> pick)
    {
        for (int i = 0; i < streaks; i++)
        {
            double angle = random.NextDouble() * 2 * Math.PI;
            double start = 40 + (random.NextDouble() * 120);
            double length = reach * (0.25 + (0.75 * random.NextDouble()));
            double width = 10 + (random.NextDouble() * 36);
            int color = pick();
            shapes.Add(Streak(random, cx, cy, angle, start, start + length, width, color, 0.9));
            if (random.NextDouble() < 0.6)
            {
                double d = start + length + 18 + (random.NextDouble() * 60);
                shapes.Add(Blob(random, cx + (Math.Cos(angle) * d), cy + (Math.Sin(angle) * d), 4 + (random.NextDouble() * (width / 2.5)), 0.15, 10, color, 0.95));
            }
        }

        shapes.Add(Blob(random, cx, cy, 70 + (random.NextDouble() * 60), 0.4, 18, pick(), 0.9) with { Color2 = pick() });
    }

    /// <summary>
    /// One splat centered on (<paramref name="cx"/>, <paramref name="cy"/>): a ragged core
    /// (a gradient to <paramref name="color2"/> when not -1), spikes with a drop at their tip,
    /// drips that run down and droplets. Under a page title, and on the corners of the panels.
    /// </summary>
    public static IReadOnlyList<PaintShape> Splash(int seed, double cx, double cy, double radius, int color, int color2 = -1)
    {
        var shapes = new List<PaintShape>();
        Splat(new Random(seed), shapes, cx, cy, radius, color, color2);
        return shapes;
    }

    /// <summary>A small round drop with a wobbly edge, centered on (<paramref name="cx"/>, <paramref name="cy"/>).</summary>
    public static PaintShape Drop(int seed, double cx, double cy, double radius, int color) =>
        Blob(new Random(seed), cx, cy, radius, 0.12, 10, color, 1);

    /// <summary>Samples the edge of a shape (for tests and hit areas), <paramref name="steps"/> points per segment.</summary>
    public static IReadOnlyList<PaintPoint> Outline(PaintShape shape, int steps = 8)
    {
        var outline = new List<PaintPoint>();
        PaintPoint from = shape.Start;
        for (int i = 0; i + 2 < shape.Segments.Count; i += 3)
        {
            PaintPoint c1 = shape.Segments[i], c2 = shape.Segments[i + 1], to = shape.Segments[i + 2];
            for (int s = 1; s <= steps; s++)
            {
                double t = (double)s / steps, u = 1 - t;
                outline.Add(new PaintPoint(
                    (u * u * u * from.X) + (3 * u * u * t * c1.X) + (3 * u * t * t * c2.X) + (t * t * t * to.X),
                    (u * u * u * from.Y) + (3 * u * u * t * c1.Y) + (3 * u * t * t * c2.Y) + (t * t * t * to.Y)));
            }

            from = to;
        }

        return outline;
    }

    /// <summary>True when (<paramref name="x"/>, <paramref name="y"/>) is inside the sampled outline.</summary>
    public static bool Contains(IReadOnlyList<PaintPoint> outline, double x, double y)
    {
        bool inside = false;
        for (int i = 0, j = outline.Count - 1; i < outline.Count; j = i++)
        {
            PaintPoint a = outline[i], b = outline[j];
            if ((a.Y > y) != (b.Y > y) && x < ((b.X - a.X) * (y - a.Y) / (b.Y - a.Y)) + a.X)
            {
                inside = !inside;
            }
        }

        return inside;
    }

    // A splat: a ragged core, spikes thrown out with a drop at their tip, droplets, and drips
    // that run down.
    private static void Splat(Random random, List<PaintShape> shapes, double cx, double cy, double r, int color, int color2)
    {
        shapes.Add(Blob(random, cx, cy, r, 0.28, 16, color, 0.95) with { Color2 = color2 });
        int spikes = 7 + random.Next(7);
        for (int i = 0; i < spikes; i++)
        {
            double angle = random.NextDouble() * 2 * Math.PI;
            double end = r * (1.25 + (random.NextDouble() * 1.1));
            shapes.Add(Streak(random, cx, cy, angle, r * 0.6, end, r * (0.12 + (random.NextDouble() * 0.14)), color, 0.95));
            if (random.NextDouble() < 0.7)
            {
                double d = end + (r * (0.12 + (random.NextDouble() * 0.3)));
                shapes.Add(Blob(random, cx + (Math.Cos(angle) * d), cy + (Math.Sin(angle) * d), r * (0.04 + (random.NextDouble() * 0.07)), 0.12, 10, color, 0.95));
            }
        }

        for (int i = 0; i < 1 + random.Next(3); i++)
        {
            shapes.Add(Drip(random, cx + ((random.NextDouble() - 0.5) * r), cy + (r * 0.55), r * (0.10 + (random.NextDouble() * 0.10)), r * (0.8 + (random.NextDouble() * 1.6)), color));
        }

        for (int i = 0; i < 10 + random.Next(12); i++)
        {
            double angle = random.NextDouble() * 2 * Math.PI, d = r * (1.2 + (random.NextDouble() * 1.6));
            shapes.Add(Blob(random, cx + (Math.Cos(angle) * d), cy + (Math.Sin(angle) * d), 2 + (random.NextDouble() * r * 0.06), 0.1, 8, color, 0.95));
        }
    }

    // Droplets of every color over the whole area.
    private static void Scatter(Random random, List<PaintShape> shapes, int count, double min, double max, double opacity, Func<int> pick)
    {
        for (int i = 0; i < count; i++)
        {
            shapes.Add(Blob(random, random.NextDouble() * Width, random.NextDouble() * Height, min + (random.NextDouble() * (max - min)), 0.12, 8, pick(), opacity));
        }
    }

    // A closed blob: points around the center at a jittered radius, smoothed.
    private static PaintShape Blob(Random random, double cx, double cy, double r, double jitter, int count, int color, double opacity)
    {
        var points = new List<PaintPoint>(count);
        double phase = random.NextDouble() * 2 * Math.PI;
        for (int i = 0; i < count; i++)
        {
            double angle = phase + (2 * Math.PI * i / count) + ((random.NextDouble() - 0.5) * (Math.PI / count));
            double radius = r * (1 + ((random.NextDouble() - 0.5) * 2 * jitter));
            points.Add(new PaintPoint(cx + (Math.Cos(angle) * radius), cy + (Math.Sin(angle) * radius)));
        }

        return Smooth(points, color, opacity);
    }

    // A streak from distance start to end along angle: wide at the start, pointed at the tip,
    // slightly bent.
    private static PaintShape Streak(Random random, double cx, double cy, double angle, double start, double end, double width, int color, double opacity)
    {
        double cos = Math.Cos(angle), sin = Math.Sin(angle);
        double bend = (random.NextDouble() - 0.5) * width * 1.5;
        PaintPoint At(double t, double side)
        {
            double along = Lerp(start, end, t);
            double across = (side * width * (1 - t) / 2) + (bend * Math.Sin(Math.PI * t));
            return new PaintPoint(cx + (cos * along) - (sin * across), cy + (sin * along) + (cos * across));
        }

        PaintPoint tip = At(1, 0);
        return Smooth([At(0, -1), At(0.35, -1), At(0.7, -1), tip, At(0.7, 1), At(0.35, 1), At(0, 1), At(-0.05, 0)], color, opacity);
    }

    // A drip: a run of paint that goes down and ends in a round drop.
    private static PaintShape Drip(Random random, double x, double top, double width, double length, int color)
    {
        double w = width / 2, wobble = (random.NextDouble() - 0.5) * width * 0.4, bulb = width * 0.75;
        double bottom = top + length;
        return Smooth(
        [
            new(x - w, top),
            new(x - (w * 0.8) + wobble, top + (length * 0.5)),
            new(x - (w * 0.7), bottom - bulb),
            new(x - bulb, bottom),
            new(x, bottom + bulb),
            new(x + bulb, bottom),
            new(x + (w * 0.7), bottom - bulb),
            new(x + (w * 0.8) + wobble, top + (length * 0.5)),
            new(x + w, top),
            new(x, top - (w * 0.6)),
        ], color, 0.95);
    }

    // Closed Catmull-Rom curve through the points, as cubic Bézier segments.
    private static PaintShape Smooth(IReadOnlyList<PaintPoint> p, int color, double opacity, int color2 = -1)
    {
        int n = p.Count;
        var segments = new List<PaintPoint>(n * 3);
        for (int i = 0; i < n; i++)
        {
            PaintPoint p0 = p[(i - 1 + n) % n], p1 = p[i], p2 = p[(i + 1) % n], p3 = p[(i + 2) % n];
            segments.Add(new PaintPoint(p1.X + ((p2.X - p0.X) / 6), p1.Y + ((p2.Y - p0.Y) / 6)));
            segments.Add(new PaintPoint(p2.X - ((p3.X - p1.X) / 6), p2.Y - ((p3.Y - p1.Y) / 6)));
            segments.Add(p2);
        }

        return new PaintShape(p[0], segments, color, opacity, color2);
    }

    // A random diagonal across (and beyond) the area: the composition never follows the grid.
    private static (PaintPoint From, PaintPoint To) Diagonal(Random random)
    {
        bool down = random.NextDouble() < 0.5;
        double x0 = -100 + (random.NextDouble() * 400), x1 = Width - 300 + (random.NextDouble() * 400);
        double yTop = -80 + (random.NextDouble() * 300), yBottom = Height - 200 + (random.NextDouble() * 280);
        return down ? (new(x0, yTop), new(x1, yBottom)) : (new(x0, yBottom), new(x1, yTop));
    }

    private static double Lerp(double a, double b, double t) => a + ((b - a) * t);

    // Stable across runs (string.GetHashCode is not): FNV-1a.
    private static int Seed(string text)
    {
        uint hash = 2166136261;
        foreach (char c in text)
        {
            hash = (hash ^ c) * 16777619;
        }

        return (int)hash;
    }
}
