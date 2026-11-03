using CleanSlate.Interop;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Data;

namespace CleanSlate.App.Converters;

/// <summary>
/// Storage category → brush of the map (D73), <c>Storage{Category}Brush</c> from the theme
/// chosen in CleanSlate (same lookup as <see cref="ConfidenceBrushConverter"/>).
/// </summary>
public sealed class StorageCategoryBrushConverter : IValueConverter
{
    /// <inheritdoc/>
    public object Convert(object value, Type targetType, object parameter, string language) =>
        Themed(value is StorageCategory category ? $"Storage{category}Brush" : "StorageOtherBrush");

    /// <inheritdoc/>
    public object ConvertBack(object value, Type targetType, object parameter, string language) =>
        throw new NotSupportedException();

    /// <summary>Resource <paramref name="key"/> of the theme chosen in CleanSlate.</summary>
    public static object Themed(string key)
    {
        var resources = Application.Current.Resources;
        if (resources.ThemeDictionaries.TryGetValue(MainWindow.CurrentThemeDictionary, out object? dictionary)
            && dictionary is ResourceDictionary theme
            && theme.TryGetValue(key, out object? themed))
        {
            return themed;
        }

        return resources.TryGetValue(key, out object? brush) ? brush : DependencyProperty.UnsetValue;
    }
}
