using CleanSlate.Interop;
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
    public async Task<ManagerDecision> ConfirmManagerAsync(ManagerSummary summary)
    {
        const string confirmWord = "CONFIRMER";
        var panel = new StackPanel { Spacing = 8, MaxWidth = 520 };
        panel.Children.Add(Text($"{summary.ItemCount} élément(s) · {summary.SizeText}", bold: true));
        foreach (string line in summary.Details)
        {
            panel.Children.Add(Text($"• {line}"));
        }

        RadioButtons? modes = null;
        if (summary.OfferModes)
        {
            modes = new RadioButtons { Header = Loader.GetString("ManagerModeHeader"), SelectedIndex = 0 };
            modes.Items.Add(Loader.GetString("ManagerModeSafe"));
            modes.Items.Add(Loader.GetString("ManagerModeImmediate"));
            panel.Children.Add(modes);
        }

        var dialog = new ContentDialog
        {
            XamlRoot = Window.Content.XamlRoot,
            Title = summary.Title,
            Content = new ScrollViewer { Content = panel },
            PrimaryButtonText = Loader.GetString("ManagerConfirmPrimary"),
            CloseButtonText = Loader.GetString("ConfirmCancel"),
            DefaultButton = ContentDialogButton.Close,
            PrimaryButtonStyle = DangerButtonStyle(),
        };

        TextBox? wordBox = null;
        if (summary.SensitiveCount > 0)
        {
            // D79: a sensitive zone, in expert mode: what it is, what can break, and the word.
            panel.Children.Add(Text(string.Format(Loader.GetString("ManagerSensitiveWarning"), summary.SensitiveCount), bold: true));
            wordBox = new TextBox { PlaceholderText = confirmWord, Header = Loader.GetString("ManagerWordPrompt") };
            wordBox.TextChanged += (_, _) => dialog.IsPrimaryButtonEnabled = wordBox.Text.Trim() == confirmWord;
            dialog.IsPrimaryButtonEnabled = false;
            panel.Children.Add(wordBox);
        }

        ContentDialogResult result = await dialog.ShowAsync();
        DeleteMode mode = modes?.SelectedIndex == 1 ? DeleteMode.Immediate : DeleteMode.Safe;
        return new ManagerDecision(result == ContentDialogResult.Primary, mode, wordBox?.Text.Trim());
    }

    /// <inheritdoc/>
    public async Task<string?> PickFolderAsync()
    {
        var picker = new Microsoft.Windows.Storage.Pickers.FolderPicker(Window.AppWindow.Id);
        var result = await picker.PickSingleFolderAsync();
        return result?.Path;
    }

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
        // Immediate deletion (D66): said plainly, never « quarantaine ».
        panel.Children.Add(summary.DeleteImmediately
            ? Text(Loader.GetString("ConfirmDeleteForGood"), bold: true)
            : Text(string.Format(Loader.GetString("ConfirmReminder"), summary.QuarantineDays)));

        var dialog = new ContentDialog
        {
            XamlRoot = Window.Content.XamlRoot,
            Title = Loader.GetString("ConfirmTitle"),
            Content = panel,
            PrimaryButtonText = Loader.GetString(summary.DeleteImmediately ? "ConfirmPrimaryDelete" : "ConfirmPrimary"),
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
