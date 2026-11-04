using CleanSlate.Presentation;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>The on-screen check of D76: every text on a plain surface, contrast at least 4.5:1.</summary>
public sealed class TextContrastTests
{
    private const int Size = 60;

    // A box of the given background, with a « letter » bar of the text color and its
    // anti-aliased edge.
    private static byte[] Image(Func<int, int, (byte R, byte G, byte B)> background, (byte R, byte G, byte B) text)
    {
        var pixels = new byte[Size * Size * 4];
        for (int y = 0; y < Size; y++)
        {
            for (int x = 0; x < Size; x++)
            {
                (byte r, byte g, byte b) = x is >= 20 and < 26 ? text : background(x, y);
                if (x == 26)
                {
                    (byte br, byte bg, byte bb) = background(x, y);
                    (r, g, b) = ((byte)((text.R + br) / 2), (byte)((text.G + bg) / 2), (byte)((text.B + bb) / 2));
                }

                int i = ((y * Size) + x) * 4;
                (pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]) = (b, g, r, 255);
            }
        }

        return pixels;
    }

    private static readonly (byte, byte, byte) Ink = (0x17, 0x21, 0x3B);

    [Fact]
    public void InkOnPaper_Passes()
    {
        TextMeasure m = TextContrast.Measure(Image((_, _) => (255, 255, 255), Ink), Size, 0, 0, Size, Size, Ink);
        Assert.True(m.Passes, $"{m.Ratio:0.0} {m.Plainness:P0}");
        Assert.Equal("#FCFCFC", m.Background);
    }

    [Fact]
    public void TextOnPaint_Fails_EvenWhenItsMainColorIsReadable()
    {
        // White paper with splashes of pink and blue behind the letters.
        (byte, byte, byte) pink = (0xFF, 0x39, 0x94), blue = (0x15, 0x9C, 0xE8), paper = (255, 255, 255);
        TextMeasure m = TextContrast.Measure(Image((x, y) => (x + y) % 9 < 3 ? pink : (x * y) % 7 == 0 ? blue : paper, Ink), Size, 0, 0, Size, Size, Ink);
        Assert.False(m.Passes);
        Assert.True(m.Plainness < TextMeasure.MinPlainness);
    }

    [Fact]
    public void TextOfLowContrast_Fails()
    {
        (byte, byte, byte) violet = (0x96, 0x38, 0xED);
        TextMeasure m = TextContrast.Measure(Image((_, _) => violet, Ink), Size, 0, 0, Size, Size, Ink);
        Assert.True(m.Plainness >= TextMeasure.MinPlainness);
        Assert.False(m.Passes); // 3.1:1
    }
}
