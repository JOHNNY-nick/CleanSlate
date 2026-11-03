using CleanSlate.Interop;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Data;

namespace CleanSlate.App.Converters;

/// <summary>
/// Confidence → badge brush from the design tokens (App.xaml):
/// ConverterParameter is <c>Foreground</c> or <c>Background</c>.
/// </summary>
public sealed class ConfidenceBrushConverter : IValueConverter
{
    /// <inheritdoc/>
    public object Convert(object value, Type targetType, object parameter, string language)
    {
        string level = value is Confidence confidence ? confidence.ToString() : nameof(Confidence.Low);
        string key = $"Confidence{level}{parameter}Brush";
        // From the theme chosen in CleanSlate: Application.Resources would answer with the one
        // of Windows' theme, so badges stayed dark under Clair and Multicolore (D72).
        var resources = Application.Current.Resources;
        if (resources.ThemeDictionaries.TryGetValue(MainWindow.CurrentThemeDictionary, out object? dictionary)
            && dictionary is ResourceDictionary theme
            && theme.TryGetValue(key, out object? themed))
        {
            return themed;
        }

        return resources.TryGetValue(key, out object? brush) ? brush : DependencyProperty.UnsetValue;
    }

    /// <inheritdoc/>
    public object ConvertBack(object value, Type targetType, object parameter, string language) =>
        throw new NotSupportedException();
}
