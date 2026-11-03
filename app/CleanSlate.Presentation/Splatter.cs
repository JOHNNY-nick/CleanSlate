namespace CleanSlate.Presentation;

/// <summary>An ellipse of a paint splash, around the splash center.</summary>
/// <param name="X">Left.</param>
/// <param name="Y">Top.</param>
/// <param name="Width">Width.</param>
/// <param name="Height">Height.</param>
/// <param name="Angle">Rotation in degrees, around its own center.</param>
public readonly record struct SplatterDot(double X, double Y, double Width, double Height, double Angle);

/// <summary>
/// Paint splashes of the Multicolore theme (D75), after the references of the owner
/// (<c>docs/design/reference-multicolore/</c>): a dense core of overlapping blots, streaks
/// thrown outwards and droplets that get smaller with the distance. Pure and seeded: the
/// same splash every time, so the window does not flicker when it is drawn again.
/// </summary>
public static class Splatter
{
    /// <summary>Farthest a droplet goes, in radii from the center.</summary>
    public const double Reach = 1.7;

    /// <summary>Ellipses of one splash centered on (<paramref name="cx"/>, <paramref name="cy"/>).</summary>
    public static IReadOnlyList<SplatterDot> Generate(int seed, double cx, double cy, double radius)
    {
        var random = new Random(seed);
        var dots = new List<SplatterDot>();
        if (radius <= 0)
        {
            return dots;
        }

        // Core: overlapping blots within half the radius.
        int blots = 7 + random.Next(4);
        for (int i = 0; i < blots; i++)
        {
            double size = radius * (0.45 + (0.45 * random.NextDouble()));
            (double x, double y) = Around(random, cx, cy, radius * 0.35);
            dots.Add(Dot(x, y, size, size * (0.7 + (0.3 * random.NextDouble())), random.NextDouble() * 180));
        }

        // Streaks: long thin ellipses pointing outwards.
        int streaks = 4 + random.Next(4);
        for (int i = 0; i < streaks; i++)
        {
            double angle = random.NextDouble() * 2 * Math.PI;
            double distance = radius * (0.55 + (0.35 * random.NextDouble()));
            double length = radius * (0.5 + (0.4 * random.NextDouble()));
            dots.Add(Dot(cx + (Math.Cos(angle) * distance), cy + (Math.Sin(angle) * distance), length, length * 0.16, angle * 180 / Math.PI));
        }

        // Droplets: smaller the farther they fly.
        int droplets = 18 + random.Next(10);
        for (int i = 0; i < droplets; i++)
        {
            double angle = random.NextDouble() * 2 * Math.PI;
            double distance = radius * (0.7 + ((Reach - 0.75) * random.NextDouble()));
            double size = Math.Max(3, radius * 0.12 * (Reach - (distance / radius)) * (0.4 + random.NextDouble()));
            dots.Add(Dot(cx + (Math.Cos(angle) * distance), cy + (Math.Sin(angle) * distance), size, size, 0));
        }

        return dots;
    }

    private static (double X, double Y) Around(Random random, double cx, double cy, double spread)
    {
        double angle = random.NextDouble() * 2 * Math.PI;
        double distance = spread * Math.Sqrt(random.NextDouble());
        return (cx + (Math.Cos(angle) * distance), cy + (Math.Sin(angle) * distance));
    }

    private static SplatterDot Dot(double centerX, double centerY, double width, double height, double angle) =>
        new(centerX - (width / 2), centerY - (height / 2), width, height, angle);
}
