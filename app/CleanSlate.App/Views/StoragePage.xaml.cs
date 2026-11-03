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
            var button = new Button
            {
                Width = Math.Max(1, tile.Width - 2),
                Height = Math.Max(1, tile.Height - 2),
                Padding = new Thickness(6, 4, 6, 4),
                HorizontalContentAlignment = HorizontalAlignment.Left,
                VerticalContentAlignment = VerticalAlignment.Top,
                Background = Brushes.Convert(tile.Category, typeof(Brush), string.Empty, string.Empty) as Brush,
                BorderBrush = text,
                BorderThickness = new Thickness(1),
                CornerRadius = new CornerRadius(4),
                Content = label,
                Tag = tile,
            };
            ToolTipService.SetToolTip(button, tile.ToolTip);
            AutomationProperties.SetName(button, tile.ToolTip.Replace('\n', ' '));
            button.Click += (_, _) => ViewModel.Open(tile);
            Canvas.SetLeft(button, tile.X + 1);
            Canvas.SetTop(button, tile.Y + 1);
            MapCanvas.Children.Add(button);
        }
    }
}
