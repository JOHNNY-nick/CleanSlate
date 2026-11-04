using Microsoft.UI.Xaml;

namespace CleanSlate.App.Converters;

/// <summary>Visibility for x:Bind function bindings, when a plain <c>bool</c> is not enough.</summary>
public static class Visible
{
    /// <summary>Visible when <paramref name="value"/> is false.</summary>
    public static Visibility Not(bool value) => value ? Visibility.Collapsed : Visibility.Visible;
}
