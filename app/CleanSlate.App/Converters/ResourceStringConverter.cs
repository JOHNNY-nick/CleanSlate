using Microsoft.UI.Xaml.Data;
using Microsoft.Windows.ApplicationModel.Resources;

namespace CleanSlate.App.Converters;

/// <summary>
/// Turns an enum value into its French text from Resources.resw: the key is
/// <c>{ConverterParameter}_{value}</c>, e.g. <c>Kind_Folder</c>.
/// </summary>
public sealed class ResourceStringConverter : IValueConverter
{
    private static readonly ResourceLoader Loader = new();

    /// <inheritdoc/>
    public object Convert(object value, Type targetType, object parameter, string language)
    {
        string key = $"{parameter}_{value}";
        string text = Loader.GetString(key);
        return string.IsNullOrEmpty(text) ? value?.ToString() ?? string.Empty : text;
    }

    /// <inheritdoc/>
    public object ConvertBack(object value, Type targetType, object parameter, string language) =>
        throw new NotSupportedException();
}
