using System.Collections.Specialized;
using CleanSlate.App.Converters;
using CleanSlate.Presentation.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace CleanSlate.App.Views;

/// <summary>
/// « Stockage » page (D73). The code-behind only draws the rectangles that the ViewModel laid
/// out (one focusable button per rectangle, read by the Narrator) and forwards clicks.
/// </summary>
public sealed partial class StoragePage : Page
{
    private static readonly StorageCategoryBrushConverter Brushes = new();

    /// <summary>Creates the page and resolves its ViewModel.</summary>
    public StoragePage()
    {
        ViewModel = App.GetService<StorageMapViewModel>();
        InitializeComponent();
        ViewModel.Tiles.CollectionChanged += OnTilesChanged;
    }

    /// <summary>Bound ViewModel.</summary>
    public StorageMapViewModel ViewModel { get; }

    private void OnLoaded(object sender, RoutedEventArgs e)
    {
        ViewModel.Load();
        ViewModel.Arrange(MapCanvas.ActualWidth, MapCanvas.Height);
    }

    private void OnMapSizeChanged(object sender, SizeChangedEventArgs e) =>
        ViewModel.Arrange(e.NewSize.Width, MapCanvas.Height);

    private void OnBreadcrumb(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.Tag is BreadcrumbItem item)
        {
            ViewModel.GoTo(item);
        }
    }

    private void OnTilesChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        MapCanvas.Children.Clear();
        foreach (StorageTile tile in ViewModel.Tiles)
        {
            var text = StorageCategoryBrushConverter.Themed("StorageTileTextBrush") as Brush;
            var label = new StackPanel { Spacing = 2, Visibility = tile.ShowLabel ? Visibility.Visible : Visibility.Collapsed };
            label.Children.Add(new TextBlock { Text = tile.Name, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, Foreground = text, TextTrimming = TextTrimming.CharacterEllipsis });
            label.Children.Add(new TextBlock { Text = tile.SizeText, Foreground = text, FontSize = 12 });
            var fill = Brushes.Convert(tile.Category, typeof(Brush), string.Empty, string.Empty) as Brush;
            var button = new Button
            {
                Width = Math.Max(1, tile.Width - 2),
                Height = Math.Max(1, tile.Height - 2),
                Padding = new Thickness(6, 4, 6, 4),
                HorizontalContentAlignment = HorizontalAlignment.Left,
                VerticalContentAlignment = VerticalAlignment.Top,
                Background = fill,
                BorderBrush = text,
                BorderThickness = new Thickness(1),
                CornerRadius = new CornerRadius(MainWindow.IsPainted ? 12 : 4),
                Content = label,
                Tag = tile,
            };
            // The category color stays under the pointer and when pressed: the Button style
            // would otherwise swap it for its neutral gray; the border thickens instead.
            foreach (string state in new[] { "PointerOver", "Pressed" })
            {
                button.Resources[$"ButtonBackground{state}"] = fill;
                button.Resources[$"ButtonForeground{state}"] = text;
                button.Resources[$"ButtonBorderBrush{state}"] = text;
            }

            double edge = MainWindow.IsPainted ? 3 : 1;
            button.PointerEntered += (_, _) => button.BorderThickness = new Thickness(edge + 2);
            button.PointerExited += (_, _) => button.BorderThickness = new Thickness(edge);
            if (MainWindow.Paint is { } paint)
            {
                Street(button, label, fill, paint);
            }

            ToolTipService.SetToolTip(button, tile.ToolTip);
            AutomationProperties.SetName(button, tile.ToolTip.Replace('\n', ' '));
            button.Click += (_, _) => ViewModel.Open(tile);
            Canvas.SetLeft(button, tile.X + 1);
            Canvas.SetTop(button, tile.Y + 1);
            MapCanvas.Children.Add(button);
        }
    }

    // Multicolore (D76), street style of the owner's mock-up: a vivid gradient, a thick ink
    // outline, and the name and size on a light, slightly tilted sticker.
    private static void Street(Button button, StackPanel label, Brush? fill, PaintDecorator paint)
    {
        if (fill is SolidColorBrush solid)
        {
            var gradient = new LinearGradientBrush { StartPoint = new Windows.Foundation.Point(0, 0), EndPoint = new Windows.Foundation.Point(1, 1) };
            gradient.GradientStops.Add(new GradientStop { Color = solid.Color, Offset = 0 });
            gradient.GradientStops.Add(new GradientStop { Color = paint.Paint(Array.IndexOf(Enumerable.Range(0, PresentationColors).Select(paint.Paint).ToArray(), solid.Color) + 1), Offset = 1 });
            button.Background = gradient;
            button.Resources["ButtonBackgroundPointerOver"] = gradient;
            button.Resources["ButtonBackgroundPressed"] = gradient;
        }

        button.BorderBrush = paint.Ink;
        button.BorderThickness = new Thickness(3);
        button.CornerRadius = new CornerRadius(14);
        button.Padding = new Thickness(8);
        button.Content = null;
        label.Margin = new Thickness(0);
        foreach (TextBlock text in label.Children.OfType<TextBlock>())
        {
            text.FontWeight = Microsoft.UI.Text.FontWeights.Black;
        }

        button.Content = new Border
        {
            Background = new SolidColorBrush(paint.ColorOf("LabelStickerColor")),
            CornerRadius = new CornerRadius(8),
            Padding = new Thickness(8, 3, 8, 4),
            RenderTransform = new RotateTransform { Angle = -2 },
            Child = label,
            Visibility = label.Visibility,
        };
    }

    private const int PresentationColors = Presentation.PaintComposer.ColorCount;
}
