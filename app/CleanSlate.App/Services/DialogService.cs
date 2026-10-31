using CleanSlate.Presentation.Services;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.Windows.ApplicationModel.Resources;

namespace CleanSlate.App.Services;

/// <summary>ContentDialog implementation of <see cref="IDialogService"/> (CleanConfirmDialog, design § 4).</summary>
public sealed class DialogService : IDialogService
{
    private const string Word = "QUARANTAINE";
    private static readonly ResourceLoader Loader = new();
    // Resolved on use, to avoid a dependency cycle with MainWindow (see NavigationService).
    private static MainWindow Window => App.GetService<MainWindow>();

    /// <inheritdoc/>
    public async Task<CleanDecision> ConfirmCleanAsync(CleanSummary summary)
    {
        var panel = new StackPanel { Spacing = 8 };
        panel.Children.Add(Text(Loader.GetString("ConfirmIntro")));
        foreach ((var kind, int count) in summary.CountByKind)
        {
            panel.Children.Add(Text($"• {count} × {Loader.GetString($"Kind_{kind}")}"));
        }

        panel.Children.Add(Text($"{Loader.GetString("ConfirmTotal")} {summary.ItemCount} · {summary.SizeText}", bold: true));
        panel.Children.Add(Text(string.Format(Loader.GetString("ConfirmReminder"), summary.QuarantineDays)));
        if (summary.Simulation)
        {
            panel.Children.Add(Text(Loader.GetString("ConfirmSimulation"), bold: true));
        }

        var dialog = new ContentDialog
        {
            XamlRoot = Window.Content.XamlRoot,
            Title = Loader.GetString("ConfirmTitle"),
            Content = panel,
            PrimaryButtonText = Loader.GetString(summary.Simulation ? "ConfirmPrimarySimulation" : "ConfirmPrimary"),
            CloseButtonText = Loader.GetString("ConfirmCancel"),
            DefaultButton = ContentDialogButton.Close,
            PrimaryButtonStyle = DangerButtonStyle(),
        };

        TextBox? wordBox = null;
        if (summary.RequiresWord)
        {
            panel.Children.Add(Text(Loader.GetString("ConfirmWordPrompt")));
            wordBox = new TextBox { PlaceholderText = Word };
            wordBox.TextChanged += (_, _) => dialog.IsPrimaryButtonEnabled = wordBox.Text.Trim() == Word;
            dialog.IsPrimaryButtonEnabled = false;
            panel.Children.Add(wordBox);
        }

        ContentDialogResult result = await dialog.ShowAsync();
        return new CleanDecision(result == ContentDialogResult.Primary, wordBox?.Text.Trim());
    }

    /// <inheritdoc/>
    public async Task<bool> ConfirmPurgeAsync(int quarantineDays)
    {
        var dialog = new ContentDialog
        {
            XamlRoot = Window.Content.XamlRoot,
            Title = Loader.GetString("PurgeTitle"),
            Content = Text(string.Format(Loader.GetString("PurgeMessage"), quarantineDays)),
            PrimaryButtonText = Loader.GetString("PurgePrimary"),
            CloseButtonText = Loader.GetString("ConfirmCancel"),
            DefaultButton = ContentDialogButton.Close,
            PrimaryButtonStyle = DangerButtonStyle(),
        };
        return await dialog.ShowAsync() == ContentDialogResult.Primary;
    }

    private static TextBlock Text(string text, bool bold = false) => new()
    {
        Text = text,
        TextWrapping = TextWrapping.Wrap,
        FontWeight = bold ? Microsoft.UI.Text.FontWeights.SemiBold : Microsoft.UI.Text.FontWeights.Normal,
    };

    /// <summary>Red is reserved for actions that move or delete (design § 2).</summary>
    private static Style DangerButtonStyle()
    {
        var style = new Style(typeof(Button));
        if (Application.Current.Resources.TryGetValue("DangerBrush", out object? brush) && brush is Brush danger)
        {
            style.Setters.Add(new Setter(Control.BackgroundProperty, danger));
            style.Setters.Add(new Setter(Control.ForegroundProperty, new SolidColorBrush(Microsoft.UI.Colors.White)));
        }

        return style;
    }
}
