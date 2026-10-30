using CleanSlate.Presentation;
using Microsoft.UI.Xaml.Data;
using Microsoft.Windows.ApplicationModel.Resources;

namespace CleanSlate.App.Converters;

/// <summary>
/// Turns an enum value into its French text from Resources.resw: the key is
/// <c>{ConverterParameter}_{value}</c>, e.g. <c>Kind_Folder</c>. Never throws (ResourceText).
/// </summary>
public sealed class ResourceStringConverter : IValueConverter
{
    private static readonly ResourceLoader Loader = new();

    /// <inheritdoc/>
    public object Convert(object value, Type targetType, object parameter, string language) =>
        ResourceText.Resolve(Loader.GetString, parameter, value);

    /// <inheritdoc/>
    public object ConvertBack(object value, Type targetType, object parameter, string language) =>
        throw new NotSupportedException();
}
