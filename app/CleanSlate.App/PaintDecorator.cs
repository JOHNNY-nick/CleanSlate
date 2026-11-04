using CleanSlate.Presentation;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Windows.Foundation;
using Windows.UI;
using Path = Microsoft.UI.Xaml.Shapes.Path;

namespace CleanSlate.App;

/// <summary>
/// Multicolore in the street style of the owner's storage map mock-up (D76): a wall covered in
/// paint, every text on an opaque paper sticker with a thick ink outline and a sharp offset
/// shadow, page titles on a tilted sticker over a splat of the page color, list rows as small
/// stickers outlined in turn in every color. <see cref="PaintComposer"/> computes the paint;
/// this class turns it into a few XAML paths (grouped by color) and dresses the page. Decoration
/// is hidden from the Narrator, never takes a click, and never moves once drawn.
/// </summary>
internal sealed class PaintDecorator
{
    /// <summary>Offset of the sticker shadows, in pixels.</summary>
    public const double ShadowOffset = 5;

    private static readonly Dictionary<(PaintStyle, string, int), IReadOnlyList<PaintShape>> Compositions = [];

    // Dominant color of each page (index of Paint0 to Paint7), as its title splat.
    private static readonly Dictionary<string, int> PageColors = new()
    {
        ["HomePage"] = 4, ["AppsPage"] = 3, ["OrphansPage"] = 1, ["HistoryPage"] = 5,
        ["StoragePage"] = 0, ["SettingsPage"] = 6, ["UninstallWizardPage"] = 7,
    };

    private readonly ResourceDictionary _palette;
    private readonly Color[] _paint;

    /// <summary>Uses the colors of <paramref name="palette"/> (Paint0 to Paint7, PaintSurfaceColor, Ink).</summary>
    public PaintDecorator(ResourceDictionary palette, PaintStyle style)
    {
        _palette = palette;
        Style = style;
        _paint = Enumerable.Range(0, PaintComposer.ColorCount).Select(i => ColorOf($"Paint{i}")).ToArray();
    }

    /// <summary>Style of the paint.</summary>
    public PaintStyle Style { get; }

    /// <summary>The paper of the stickers: plain and opaque, for every text.</summary>
    public Brush Paper => new SolidColorBrush(ColorOf("PaintSurfaceColor"));

    /// <summary>The ink of the outlines and shadows.</summary>
    public Brush Ink => new SolidColorBrush(ColorOf("Ink"));

    /// <summary>A paint color, in turn.</summary>
    public Color Paint(int index) => _paint[((index % _paint.Length) + _paint.Length) % _paint.Length];

    /// <summary>A color of the palette (transparent when missing).</summary>
    public Color ColorOf(string key) => _palette.TryGetValue(key, out object? value) && value is Color color ? color : Microsoft.UI.Colors.Transparent;

    /// <summary>Dominant color of a page.</summary>
    public static int DominantOf(string page) => PageColors.TryGetValue(page, out int color) ? color : 4;

    /// <summary>The paint wall of <paramref name="page"/>, stretched over the whole window and cropped by it.</summary>
    public UIElement Background(string page)
    {
        int dominant = DominantOf(page);
        if (!Compositions.TryGetValue((Style, page, dominant), out IReadOnlyList<PaintShape>? shapes))
        {
            shapes = PaintComposer.Compose(Style, page, dominant);
            Compositions[(Style, page, dominant)] = shapes;
        }

        var canvas = new Canvas { Width = PaintComposer.Width, Height = PaintComposer.Height };
        Draw(canvas, shapes);
        return Decoration(new Viewbox { Stretch = Stretch.UniformToFill, Child = canvas });
    }

    /// <summary>
    /// Adds the shapes to <paramref name="canvas"/>. Plain shapes of one color and opacity share
    /// one path (a few dozen elements for thousands of drops); gradients get their own.
    /// </summary>
    public void Draw(Canvas canvas, IEnumerable<PaintShape> shapes, Color? color = null)
    {
        var groups = new Dictionary<(int, double), PathGeometry>();
        foreach (PaintShape shape in shapes)
        {
            if (shape.Color2 >= 0 && color is null)
            {
                var single = new PathGeometry();
                single.Figures.Add(Figure(shape));
                var brush = new LinearGradientBrush { StartPoint = new Point(0, 0), EndPoint = new Point(1, 1) };
                brush.GradientStops.Add(new GradientStop { Color = Paint(shape.Color), Offset = 0 });
                brush.GradientStops.Add(new GradientStop { Color = Paint(shape.Color2), Offset = 1 });
                canvas.Children.Add(new Path { Data = single, Fill = brush, Opacity = shape.Opacity });
                continue;
            }

            var key = (color is null ? shape.Color : -1, Math.Round(shape.Opacity, 2));
            if (!groups.TryGetValue(key, out PathGeometry? geometry))
            {
                geometry = new PathGeometry { FillRule = FillRule.Nonzero };
                groups[key] = geometry;
                canvas.Children.Add(new Path
                {
                    Data = geometry,
                    Fill = new SolidColorBrush(color ?? Paint(shape.Color)),
                    Opacity = shape.Opacity,
                });
            }

            geometry.Figures.Add(Figure(shape));
        }
    }

    /// <summary>
    /// Dresses <paramref name="page"/> (once): panels and loose texts become stickers, the title
    /// a tilted sticker over a splat, list rows small stickers, main buttons green.
    /// </summary>
    public void Decorate(Page page)
    {
        if (page.Tag is "painted" || page.Content is not UIElement content)
        {
            return;
        }

        page.Tag = "painted";
        Panel? host = content switch
        {
            ScrollViewer { Content: Panel scrolled } viewer => Wrap(viewer, scrolled),
            Panel panel => panel,
            _ => null,
        };
        if (host is null)
        {
            return;
        }

        // Splats, shadows and the paper of loose texts: under the whole content, scrolling with it.
        var decor = Decoration(new Canvas());
        Grid.SetRowSpan(decor, 99);
        Grid.SetColumnSpan(decor, 99);
        host.Children.Insert(0, decor);

        var stickers = new List<FrameworkElement>();
        var loose = new List<FrameworkElement>();
        var titles = new List<Border>();
        Classify(host, decor, stickers, loose, titles);
        foreach (Border title in titles)
        {
            Title(title);
        }

        // Buttons and list rows once the page is laid out: the moved panel has its visual tree again.
        page.DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, () =>
        {
            DressButtons(page);
            DressLists(page);
        });
        int color = DominantOf(page.GetType().Name);
        bool pending = false;
        void Schedule()
        {
            if (!pending)
            {
                pending = true;
                page.DispatcherQueue.TryEnqueue(() =>
                {
                    pending = false;
                    DrawDecor(decor, stickers, loose, titles, color);
                });
            }
        }

        host.SizeChanged += (_, _) => Schedule();
        foreach (FrameworkElement element in stickers.Concat(loose).Concat(titles))
        {
            element.SizeChanged += (_, _) => Schedule();
        }

        Schedule();
        if (content is ScrollViewer scroll)
        {
            scroll.ChangeView(null, 0, null, disableAnimation: true); // the wrapping kept no offset to show
        }
    }

    // A ScrollViewer's panel becomes a grid holding the decoration under it.
    private static Panel Wrap(ScrollViewer viewer, Panel scrolled)
    {
        viewer.Content = null;
        var grid = new Grid();
        grid.Children.Add(scrolled);
        viewer.Content = grid;
        return grid;
    }

    // Cards become stickers; titles are collected; panels are walked; any other element (text,
    // list, control) gets a paper sticker drawn under it.
    private void Classify(Panel panel, Canvas decor, List<FrameworkElement> stickers, List<FrameworkElement> loose, List<Border> titles)
    {
        var resources = Application.Current.Resources;
        Style? card = resources.TryGetValue("CardBorderStyle", out object? c) ? c as Style : null;
        Style? header = resources.TryGetValue("PageHeaderBorderStyle", out object? h) ? h as Style : null;
        foreach (UIElement child in panel.Children.ToList())
        {
            if (child == decor || child is not FrameworkElement element)
            {
                continue;
            }

            if (element is Border border && ReferenceEquals(border.Style, card))
            {
                border.Background = Paper;
                border.BorderBrush = Ink;
                border.BorderThickness = new Thickness(3);
                border.CornerRadius = new CornerRadius(18);
                stickers.Add(border);
            }
            else if (element is Border title && ReferenceEquals(title.Style, header))
            {
                titles.Add(title);
            }
            else if (element is Panel inner && inner.Background is null)
            {
                Classify(inner, decor, stickers, loose, titles);
            }
            else
            {
                loose.Add(element);
            }
        }
    }

    // The title: a paper sticker with an ink outline, tilted, in big black letters.
    private void Title(Border header)
    {
        header.Background = Paper;
        header.BorderBrush = Ink;
        header.BorderThickness = new Thickness(3);
        header.CornerRadius = new CornerRadius(14);
        header.Padding = new Thickness(20, 6, 20, 8);
        header.Margin = new Thickness(18, 22, 0, 30);
        header.HorizontalAlignment = HorizontalAlignment.Left;
        header.RenderTransform = new RotateTransform { Angle = -2 };
        if (header.Child is TextBlock text)
        {
            text.Foreground = new SolidColorBrush(ColorOf("PageHeaderForegroundBrush"));
            text.FontWeight = Microsoft.UI.Text.FontWeights.Black;
        }
    }

    // Under the content, in this order: the splat of each title, a splat on a corner of each
    // sticker (in turn in every color, running past its edge), the offset ink shadows, then the
    // paper of the loose texts with its outline.
    private void DrawDecor(Canvas decor, List<FrameworkElement> stickers, List<FrameworkElement> loose, List<Border> titles, int color)
    {
        decor.Children.Clear();
        var splats = new Canvas();
        decor.Children.Add(splats);
        foreach (Border title in titles.Where(Shown))
        {
            Rect box = Bounds(title, decor);
            Draw(splats, PaintComposer.Splash(31, box.X + (box.Width * 0.55), box.Y + (box.Height * 0.55), Math.Max(box.Height * 1.1, 46), color, (color + 2) % PaintComposer.ColorCount));
            AddShadow(decor, title, 14);
        }

        var papers = Merge(loose.Where(Shown).Select(e =>
        {
            Rect r = Bounds(e, decor);
            return new Rect(r.X - 14, r.Y - 8, r.Width + 28, r.Height + 16);
        }).ToList(), gap: 20);

        int turn = color + 1;
        foreach (FrameworkElement sticker in stickers.Where(Shown))
        {
            Rect box = Bounds(sticker, decor);
            Corner(splats, box, turn++);
            AddShadow(decor, sticker, 18);
        }

        foreach (Rect paper in papers)
        {
            Corner(splats, paper, turn++);
            var shadow = Rounded(paper.Width, paper.Height, 18, Ink, null);
            Canvas.SetLeft(shadow, paper.X + ShadowOffset);
            Canvas.SetTop(shadow, paper.Y + ShadowOffset);
            decor.Children.Add(shadow);
        }

        foreach (Rect paper in papers)
        {
            var sheet = Rounded(paper.Width, paper.Height, 18, Paper, Ink);
            Canvas.SetLeft(sheet, paper.X);
            Canvas.SetTop(sheet, paper.Y);
            decor.Children.Add(sheet);
        }
    }

    // A splat on one corner of a box, half outside it: the paint runs past the sticker's edge.
    private void Corner(Canvas canvas, Rect box, int turn)
    {
        bool right = turn % 2 == 0, bottom = turn % 3 == 0;
        double x = right ? box.Right - 6 : box.Left + 6, y = bottom ? box.Bottom - 6 : box.Top + 6;
        Draw(canvas, PaintComposer.Splash(turn * 13, x, y, 16 + (turn % 4 * 5), turn % PaintComposer.ColorCount));
    }

    // The sharp offset shadow of an element, with its tilt.
    private void AddShadow(Canvas decor, FrameworkElement element, double radius)
    {
        Point origin = element.TransformToVisual(decor).TransformPoint(new Point(0, 0));
        var shadow = Rounded(element.ActualWidth, element.ActualHeight, radius, Ink, null);
        if (element.RenderTransform is RotateTransform rotate)
        {
            shadow.RenderTransform = new RotateTransform { Angle = rotate.Angle };
        }

        Canvas.SetLeft(shadow, origin.X + ShadowOffset);
        Canvas.SetTop(shadow, origin.Y + ShadowOffset);
        decor.Children.Add(shadow);
    }

    // Main buttons (MainButtonStyle) in green with an ink outline: WinUI builds their brush
    // once from the system accent, so the colors are given to each button.
    private void DressButtons(DependencyObject root)
    {
        if (!Application.Current.Resources.TryGetValue("MainButtonStyle", out object? found) || found is not Style style)
        {
            return;
        }

        foreach (Button button in Descendants(root).OfType<Button>().Where(b => ReferenceEquals(b.Style, style)))
        {
            button.Background = new SolidColorBrush(ColorOf("AccentButtonBackground"));
            button.Foreground = new SolidColorBrush(ColorOf("AccentButtonForeground"));
            button.BorderBrush = Ink;
            button.BorderThickness = new Thickness(2);
            button.FontWeight = Microsoft.UI.Text.FontWeights.Bold;
            foreach (string state in new[] { string.Empty, "PointerOver", "Pressed" })
            {
                button.Resources[$"AccentButtonBackground{state}"] = new SolidColorBrush(ColorOf($"AccentButtonBackground{state}"));
                button.Resources[$"AccentButtonForeground{state}"] = new SolidColorBrush(ColorOf($"AccentButtonForeground{state}"));
                button.Resources[$"AccentButtonBorderBrush{state}"] = Ink;
            }
        }
    }

    // List rows become small paper stickers outlined in turn in every color. Done as each row
    // is realized: long lists stay virtualized and fluid.
    private void DressLists(DependencyObject root)
    {
        foreach (ListView list in Descendants(root).OfType<ListView>())
        {
            // A new row style regenerates every row, including those already shown.
            var style = new Style(typeof(ListViewItem));
            foreach (Setter setter in list.ItemContainerStyle?.Setters.OfType<Setter>() ?? [])
            {
                style.Setters.Add(new Setter(setter.Property, setter.Value));
            }

            if (!style.Setters.OfType<Setter>().Any(x => x.Property == Control.HorizontalContentAlignmentProperty))
            {
                // The columns of a row keep the whole width.
                style.Setters.Add(new Setter(Control.HorizontalContentAlignmentProperty, HorizontalAlignment.Stretch));
            }

            style.Setters.Add(new Setter(Control.BackgroundProperty, Paper));
            style.Setters.Add(new Setter(Control.BorderThicknessProperty, new Thickness(3)));
            style.Setters.Add(new Setter(Control.CornerRadiusProperty, new CornerRadius(14)));
            style.Setters.Add(new Setter(FrameworkElement.MarginProperty, new Thickness(2, 4, 10, 4)));
            list.ContainerContentChanging += (_, e) =>
            {
                if (e.ItemContainer is SelectorItem row)
                {
                    row.BorderBrush = new SolidColorBrush(Paint(e.ItemIndex + 2));
                }
            };
            list.ItemContainerStyle = style;
        }
    }

    private static bool Shown(FrameworkElement e) =>
        e.Visibility == Visibility.Visible && e.ActualWidth > 0 && e.ActualHeight > 0 && e.XamlRoot is not null;

    private static Rect Bounds(FrameworkElement e, UIElement to) =>
        e.TransformToVisual(to).TransformBounds(new Rect(0, 0, e.ActualWidth, e.ActualHeight));

    private static Border Rounded(double width, double height, double radius, Brush fill, Brush? outline) => new()
    {
        Width = Math.Max(0, width),
        Height = Math.Max(0, height),
        CornerRadius = new CornerRadius(radius),
        Background = fill,
        BorderBrush = outline,
        BorderThickness = new Thickness(outline is null ? 0 : 3),
    };

    // Unites boxes that overlap or are less than gap apart: no thin strip of paint between them.
    private static List<Rect> Merge(List<Rect> boxes, double gap)
    {
        bool merged = true;
        while (merged)
        {
            merged = false;
            for (int i = 0; i < boxes.Count && !merged; i++)
            {
                for (int j = i + 1; j < boxes.Count && !merged; j++)
                {
                    Rect a = boxes[i], b = boxes[j];
                    if (a.X <= b.X + b.Width + gap && b.X <= a.X + a.Width + gap && a.Y <= b.Y + b.Height + gap && b.Y <= a.Y + a.Height + gap)
                    {
                        a.Union(b);
                        boxes[i] = a;
                        boxes.RemoveAt(j);
                        merged = true;
                    }
                }
            }
        }

        return boxes;
    }

    /// <summary>Visual descendants of <paramref name="parent"/>.</summary>
    public static IEnumerable<DependencyObject> Descendants(DependencyObject parent)
    {
        int count = VisualTreeHelper.GetChildrenCount(parent);
        for (int i = 0; i < count; i++)
        {
            DependencyObject child = VisualTreeHelper.GetChild(parent, i);
            yield return child;
            foreach (DependencyObject below in Descendants(child))
            {
                yield return below;
            }
        }
    }

    /// <summary>Decoration only: no click, nothing read by the Narrator.</summary>
    public static T Decoration<T>(T element)
        where T : UIElement
    {
        element.IsHitTestVisible = false;
        AutomationProperties.SetAccessibilityView(element, AccessibilityView.Raw);
        return element;
    }

    private static PathFigure Figure(PaintShape shape)
    {
        var figure = new PathFigure { StartPoint = new Point(shape.Start.X, shape.Start.Y), IsClosed = true, IsFilled = true };
        for (int i = 0; i + 2 < shape.Segments.Count; i += 3)
        {
            figure.Segments.Add(new BezierSegment
            {
                Point1 = new Point(shape.Segments[i].X, shape.Segments[i].Y),
                Point2 = new Point(shape.Segments[i + 1].X, shape.Segments[i + 1].Y),
                Point3 = new Point(shape.Segments[i + 2].X, shape.Segments[i + 2].Y),
            });
        }

        return figure;
    }
}
