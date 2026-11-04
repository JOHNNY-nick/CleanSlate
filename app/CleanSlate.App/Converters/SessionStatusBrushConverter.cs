using CleanSlate.Interop;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Data;
using Microsoft.UI.Xaml.Media;

namespace CleanSlate.App.Converters;

/// <summary>
/// State of a session → its pill (D76): in Multicolore a colored pill with ink text
/// (ConverterParameter <c>Background</c> or <c>Foreground</c>); in the other themes no pill and
/// the usual secondary text, so that Clair and Sombre do not change.
/// </summary>
public sealed class SessionStatusBrushConverter : IValueConverter
{
    /// <inheritdoc/>
    public object Convert(object value, Type targetType, object parameter, string language)
    {
        bool background = parameter as string == "Background";
        if (MainWindow.Paint is { } paint)
        {
            string state = value is SessionStatus status ? status.ToString() : nameof(SessionStatus.Completed);
            return new SolidColorBrush(background ? paint.ColorOf($"Status{state}Color") : paint.ColorOf("Ink"));
        }

        return background
            ? new SolidColorBrush(Microsoft.UI.Colors.Transparent)
            : StorageCategoryBrushConverter.Themed("TextSecondaryBrush");
    }

    /// <inheritdoc/>
    public object ConvertBack(object value, Type targetType, object parameter, string language) =>
        throw new NotSupportedException();
}
