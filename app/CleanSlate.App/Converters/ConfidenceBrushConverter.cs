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
        return Application.Current.Resources.TryGetValue(key, out object? brush) ? brush : DependencyProperty.UnsetValue;
    }

    /// <inheritdoc/>
    public object ConvertBack(object value, Type targetType, object parameter, string language) =>
        throw new NotSupportedException();
}
