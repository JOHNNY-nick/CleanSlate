using System.Xml.Linq;
using Xunit;

namespace CleanSlate.Tests;

/// <summary>
/// Layout of step 4 of the uninstall wizard (D57), read from the XAML source: the two defects
/// seen on 30 Oct. 2026 must not come back. (1) « la commande ci-dessous » was shown under the
/// command; (2) lines of the progress history were cut on the right instead of wrapping.
/// </summary>
public sealed class UninstallWizardLayoutTests
{
    private static readonly XNamespace Xaml = "http://schemas.microsoft.com/winfx/2006/xaml/presentation";
    private static readonly XNamespace X = "http://schemas.microsoft.com/winfx/2006/xaml";

    private static string RepoFile(string relative)
    {
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir is not null; dir = dir.Parent)
        {
            var path = Path.Combine(dir.FullName, relative);
            if (File.Exists(path))
            {
                return path;
            }
        }
        throw new FileNotFoundException(relative);
    }

    private static XDocument Page() =>
        XDocument.Load(RepoFile(Path.Combine("app", "CleanSlate.App", "Views", "UninstallWizardPage.xaml")));

    private static string? Uid(XElement e) => (string?)e.Attribute(X + "Uid");

    private static XElement Step4(XDocument page) =>
        page.Descendants(Xaml + "Border").Single(b => ((string?)b.Attribute("Visibility") ?? "").Contains("IsStep4"));

    [Fact]
    public void SimulationSentence_ComesBeforeTheCommandItSaysIsBelow()
    {
        var elements = Step4(Page()).Descendants().ToList();
        var sentence = elements.FindIndex(e => Uid(e) == "WizardSimulatedDone");
        var command = elements.FindIndex(e => Uid(e) == "WizardCommand");
        Assert.True(sentence >= 0 && command >= 0);
        Assert.True(sentence < command, "« ci-dessous » must be above the command line");

        var resources = XDocument.Load(RepoFile(Path.Combine("app", "CleanSlate.App", "Strings", "fr-FR", "Resources.resw")));
        var text = resources.Descendants("data").Single(d => (string?)d.Attribute("name") == "WizardSimulatedDone.Text").Element("value")!.Value;
        Assert.Contains("ci-dessous", text);
    }

    [Fact]
    public void HistoryLines_WrapAtAnyWidth()
    {
        var page = Page();
        var template = page.Descendants(Xaml + "DataTemplate").Single(t => (string?)t.Attribute(X + "Key") == "ProgressEntryTemplate");
        var blocks = template.Descendants(Xaml + "TextBlock").ToList();
        Assert.NotEmpty(blocks);
        Assert.All(blocks, b => Assert.Equal("Wrap", (string?)b.Attribute("TextWrapping")));
        // An Auto column would take the width of its text and push the rest out of view.
        Assert.DoesNotContain(template.Descendants(Xaml + "ColumnDefinition"), c => (string?)c.Attribute("Width") == "Auto");
    }

    [Fact]
    public void NoScrollViewer_GivesItsContentAnInfiniteWidth()
    {
        var viewers = Page().Descendants(Xaml + "ScrollViewer").ToList();
        Assert.NotEmpty(viewers);
        Assert.All(viewers, v => Assert.Equal("Disabled", (string?)v.Attribute("HorizontalScrollBarVisibility")));
    }

    [Fact]
    public void Step4Sentences_Wrap()
    {
        // Short titles excepted; every sentence, count or command line wraps.
        string[] titles = ["WizardDone", "WizardHistoryTitle"];
        var blocks = Step4(Page()).Descendants(Xaml + "TextBlock")
            .Where(b => !titles.Contains(Uid(b)) && !b.Ancestors(Xaml + "DataTemplate").Any())
            .ToList();
        Assert.NotEmpty(blocks);
        Assert.All(blocks, b => Assert.Equal("Wrap", (string?)b.Attribute("TextWrapping")));
    }
}
