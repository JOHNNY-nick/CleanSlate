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
    }

    /// <summary>Bound ViewModel.</summary>
    public StorageMapViewModel ViewModel { get; }

    // The ViewModel outlives the page (D82): the page listens to it only while it is shown.
    private void OnLoaded(object sender, RoutedEventArgs e)
    {
        Bindings.Update(); // lists bound again if the page was left and shown anew
        ViewModel.Tiles.CollectionChanged -= OnTilesChanged;
        ViewModel.Tiles.CollectionChanged += OnTilesChanged;
        ViewModel.Load();
        ViewModel.Arrange(MapCanvas.ActualWidth, MapCanvas.Height);
    }

    // A line of the explorer under the map (D83): a folder opens on both sides, a file is checked.
    private async void OnEntryClick(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is ExplorerRow row)
        {
            await ViewModel.OpenEntryAsync(row);
        }
    }

    private void OnUnloaded(object sender, RoutedEventArgs e)
    {
        ViewModel.Tiles.CollectionChanged -= OnTilesChanged;
        Bindings.StopTracking();
        BoundLists.Release(this);
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
            // A small rectangle shows its name alone, in a smaller font (D81).
            label.Children.Add(new TextBlock { Text = tile.Name, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, Foreground = text, TextTrimming = TextTrimming.CharacterEllipsis, FontSize = tile.ShowSize ? 14 : 11 });
            if (tile.ShowSize)
            {
                label.Children.Add(new TextBlock { Text = tile.SizeText, Foreground = text, FontSize = 12 });
            }
            var fill = Brushes.Convert(tile.Category, typeof(Brush), string.Empty, string.Empty) as Brush;
            var button = new Button
            {
                Width = Math.Max(1, tile.Width - 2),
                Height = Math.Max(1, tile.Height - 2),
                Padding = tile.ShowSize ? new Thickness(6, 4, 6, 4) : new Thickness(3, 1, 3, 1),
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
                Street(button, label, fill, paint, compact: !tile.ShowSize);
            }

            if (tile.IsLoading)
            {
                // A level computed on its own (D82): a sign in the rectangle, never an error.
                var loading = new Grid();
                if (button.Content is UIElement content)
                {
                    button.Content = null;
                    loading.Children.Add(content);
                }

                loading.Children.Add(new ProgressRing { IsActive = true, Width = 24, Height = 24, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center, Foreground = text });
                button.HorizontalContentAlignment = HorizontalAlignment.Stretch;
                button.VerticalContentAlignment = VerticalAlignment.Stretch;
                button.Content = loading;
            }

            ToolTipService.SetToolTip(button, tile.IsLoading ? tile.ToolTip + "\n" + Texts.Get("StorageLoading") : tile.ToolTip);
            AutomationProperties.SetName(button, tile.ToolTip.Replace('\n', ' '));
            button.Click += async (_, _) => await ViewModel.OpenAsync(tile);
            Canvas.SetLeft(button, tile.X + 1);
            Canvas.SetTop(button, tile.Y + 1);
            MapCanvas.Children.Add(button);
        }
    }

    // Multicolore (D76), street style of the owner's mock-up: a vivid gradient, a thick ink
    // outline, and the name and size on a light, slightly tilted sticker.
    private static void Street(Button button, StackPanel label, Brush? fill, PaintDecorator paint, bool compact)
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
        button.CornerRadius = new CornerRadius(compact ? 8 : 14);
        button.Padding = compact ? new Thickness(3) : new Thickness(8);
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
            Padding = compact ? new Thickness(4, 1, 4, 1) : new Thickness(8, 3, 8, 4),
            // Upright: WinUI draws a rotated text blurred (owner, 4 Nov. 2026: « mets-la en 4K »).
            Child = label,
            Visibility = label.Visibility,
        };
    }

    private const int PresentationColors = Presentation.PaintComposer.ColorCount;
}
