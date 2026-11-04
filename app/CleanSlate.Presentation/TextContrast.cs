namespace CleanSlate.Presentation;

/// <summary>What is under a text on screen: its contrast and how plain the background is.</summary>
/// <param name="Ratio">WCAG contrast between the text color and the main background color.</param>
/// <param name="Plainness">Share of the background pixels that are that main color (1 = plain).</param>
/// <param name="Background">Main background color, « #RRGGBB ».</param>
public readonly record struct TextMeasure(double Ratio, double Plainness, string Background)
{
    /// <summary>Lowest contrast allowed for text (WCAG AA).</summary>
    public const double MinRatio = 4.5;

    /// <summary>Lowest plainness: no paint, stripe or pattern behind the letters.</summary>
    public const double MinPlainness = 0.9;

    /// <summary>The text sits on a plain surface and is readable.</summary>
    public bool Passes => Ratio >= MinRatio && Plainness >= MinPlainness;
}

/// <summary>
/// Measures, on the pixels of a screenshot, the background behind a text (D76): the check of
/// « every text on a plain surface, contrast at least 4.5:1 », run on every page captured.
/// Pixels close to the text color, and the anti-aliased ones between text and background, are
/// the letters; the rest is the background, whose main color must cover nearly all of it.
/// </summary>
public static class TextContrast
{
    /// <summary>Measures the box (in pixels) of a <paramref name="bgra"/> image of <paramref name="width"/> pixels per row.</summary>
    public static TextMeasure Measure(byte[] bgra, int width, int left, int top, int boxWidth, int boxHeight, (byte R, byte G, byte B) text)
    {
        var background = new List<(int R, int G, int B)>();
        var buckets = new Dictionary<int, int>();
        int height = bgra.Length / 4 / width;
        for (int y = Math.Max(0, top); y < Math.Min(height, top + boxHeight); y++)
        {
            for (int x = Math.Max(0, left); x < Math.Min(width, left + boxWidth); x++)
            {
                int i = ((y * width) + x) * 4;
                (int r, int g, int b) = (bgra[i + 2], bgra[i + 1], bgra[i]);
                if (Distance((r, g, b), (text.R, text.G, text.B)) < 70)
                {
                    continue; // a letter
                }

                background.Add((r, g, b));
                int key = ((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3);
                buckets[key] = buckets.TryGetValue(key, out int n) ? n + 1 : 1;
            }
        }

        if (background.Count == 0)
        {
            return new TextMeasure(1, 0, "#000000");
        }

        int main = buckets.MaxBy(kv => kv.Value).Key;
        (int R, int G, int B) dominant = (((main >> 10) & 31) << 3 | 4, ((main >> 5) & 31) << 3 | 4, (main & 31) << 3 | 4);
        int plain = 0, counted = 0;
        foreach ((int R, int G, int B) p in background)
        {
            if (Distance(p, dominant) < 28)
            {
                plain++;
                counted++;
            }
            else if (!Between(p, (text.R, text.G, text.B), dominant))
            {
                counted++; // neither background nor the edge of a letter: something under the text
            }
        }

        string hex = $"#{dominant.R:X2}{dominant.G:X2}{dominant.B:X2}";
        return new TextMeasure(ThemePalette.Contrast($"#{text.R:X2}{text.G:X2}{text.B:X2}", hex), counted == 0 ? 1 : (double)plain / counted, hex);
    }

    private static double Distance((int R, int G, int B) a, (int R, int G, int B) b) =>
        Math.Sqrt(((a.R - b.R) * (a.R - b.R)) + ((a.G - b.G) * (a.G - b.G)) + ((a.B - b.B) * (a.B - b.B)));

    // On the segment from the text color to the background color: an anti-aliased edge.
    private static bool Between((int R, int G, int B) p, (int R, int G, int B) a, (int R, int G, int B) b)
    {
        double dx = b.R - a.R, dy = b.G - a.G, dz = b.B - a.B;
        double length2 = (dx * dx) + (dy * dy) + (dz * dz);
        if (length2 == 0)
        {
            return false;
        }

        double t = Math.Clamp((((p.R - a.R) * dx) + ((p.G - a.G) * dy) + ((p.B - a.B) * dz)) / length2, 0, 1);
        double x = a.R + (t * dx) - p.R, y = a.G + (t * dy) - p.G, z = a.B + (t * dz) - p.B;
        return Math.Sqrt((x * x) + (y * y) + (z * z)) < 24;
    }
}
