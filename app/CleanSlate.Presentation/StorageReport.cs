using System.Globalization;
using System.Text;
using CleanSlate.Interop;
using CleanSlate.Presentation.ViewModels;

namespace CleanSlate.Presentation;

/// <summary>
/// The report of an analysis of the Gestionnaire (D85), as a printable page in the visual style
/// of the owner's book (<c>.reference\Lyrics-Vocab-le-livre.pdf</c>): a vivid violet cover,
/// numbered sections in violet, tables with a lavender header, pink and red call-outs, paths in
/// code chips. The application prints it to PDF (Edge engine, WebView2); fonts of Windows.
/// </summary>
public static class StorageReport
{
    /// <summary>Most of the largest files listed.</summary>
    public const int LargestShown = 100;

    /// <summary>Most of the old files listed.</summary>
    public const int OldShown = 50;

    private static readonly CultureInfo French = CultureInfo.GetCultureInfo("fr-FR");

    // Colors of the book: violet of the cover and of the titles, ink, lavender of the tables,
    // pink and red of the call-outs.
    private const string Style = """
        @page { size: A4; margin: 18mm 16mm 18mm 16mm; }
        @page :first { margin: 0; }
        * { box-sizing: border-box; }
        body { margin: 0; color: #1f1a3d; font-family: "Segoe UI Variable Text", "Segoe UI", sans-serif; font-size: 10.5pt; line-height: 1.55; }
        h1, h2, h3, .title { font-family: "Segoe UI Variable Display", "Segoe UI", sans-serif; font-weight: 700; letter-spacing: -0.01em; }
        .cover { height: 297mm; padding: 0 22mm; background: #6c4dff; color: #fff; display: flex; flex-direction: column; justify-content: center; page-break-after: always; }
        .cover .over { font-size: 9.5pt; font-weight: 700; letter-spacing: 0.14em; text-transform: uppercase; opacity: 0.9; }
        .cover .title { font-size: 34pt; line-height: 1.12; margin: 14px 0 22px; }
        .cover .lead { font-size: 14pt; line-height: 1.45; max-width: 150mm; }
        .cover .meta { margin-top: 48px; font-size: 10pt; opacity: 0.9; }
        h2 { color: #5b3fe8; font-size: 19pt; margin: 0 0 12px; page-break-after: avoid; }
        h2 .n { margin-right: 8px; }
        section { margin-bottom: 26px; }
        p { margin: 0 0 10px; }
        table { width: 100%; border-collapse: collapse; margin: 8px 0 14px; font-size: 9.5pt; page-break-inside: auto; }
        tr { page-break-inside: avoid; }
        th { background: #efebff; color: #5b3fe8; font-weight: 700; text-align: left; }
        th, td { border: 1px solid #e2def5; padding: 6px 8px; vertical-align: top; }
        td.size, th.size { text-align: right; white-space: nowrap; }
        code { font-family: "Cascadia Mono", Consolas, monospace; font-size: 8.5pt; background: #f1eefb; border-radius: 4px; padding: 1px 4px; word-break: break-all; }
        .callout { border-left: 5px solid #d6246e; background: #ffe4ee; border-radius: 6px; padding: 12px 16px; margin: 12px 0 16px; page-break-inside: avoid; }
        .callout.safety { border-left-color: #d9283c; background: #ffe3e3; }
        .callout .head { font-weight: 700; margin-bottom: 6px; }
        .callout.safety .head { color: #d9283c; }
        .badge { display: inline-block; font-size: 8pt; font-weight: 700; border-radius: 4px; padding: 0 6px; white-space: nowrap; }
        .safe { background: #dcf6e6; color: #146c3a; }
        .check { background: #fff0cc; color: #7a5200; }
        .sensitive { background: #ffe3e3; color: #a3172a; }
        """;

    /// <summary>
    /// The whole report: cover, diagnosis, what can be freed, categories, largest and old files.
    /// </summary>
    public static string Html(StorageAnalysis analysis, IEnumerable<string> sentences, IEnumerable<RecoverableLine> recoverable, DateTimeOffset generatedAt)
    {
        var html = new StringBuilder();
        html.Append("<!doctype html><html lang=\"fr\"><head><meta charset=\"utf-8\"><title>Rapport de stockage CleanSlate</title><style>")
            .Append(Style).Append("</style></head><body>");

        // Cover.
        html.Append("<div class=\"cover\"><div class=\"over\">CleanSlate · rapport de stockage</div>")
            .Append("<div class=\"title\">Ce qui occupe ").Append(E(analysis.Root.TrimEnd('\\'))).Append("<br>et ce qui peut être libéré</div>")
            .Append("<div class=\"lead\">Le résultat de l'analyse du disque : les plus gros consommateurs, ce qui peut être libéré sans risque et ce qu'il faut vérifier avant de supprimer.</div>")
            .Append("<div class=\"meta\">")
            .Append(E(string.Format(French, "Généré le {0} à {1:HH:mm} · {2:N0} fichiers · {3} analysés", Formats.Date(DateOnly.FromDateTime(generatedAt.LocalDateTime)), generatedAt.LocalDateTime, analysis.Files, Formats.Size(analysis.SizeBytes))))
            .Append("</div></div>");

        int number = 0;
        string Section(string title) => $"<section><h2><span class=\"n\">{++number}</span>{E(title)}</h2>";

        // 1. Diagnosis.
        html.Append(Section("Diagnostic"));
        foreach (string sentence in sentences)
        {
            html.Append("<p>").Append(E(sentence)).Append("</p>");
        }

        if (analysis.Denied > 0)
        {
            html.Append("<p>").Append(E(string.Format(French, "{0:N0} dossier(s) n'ont pas pu être lus (accès refusé) : ils ne sont pas comptés.", analysis.Denied))).Append("</p>");
        }

        html.Append("</section>");

        // 2. What can be freed.
        List<RecoverableLine> lines = recoverable.Where(r => r.SizeBytes > 0).ToList();
        if (lines.Count > 0)
        {
            ulong safe = lines.Where(r => r.Safe).Aggregate(0UL, (s, r) => s + r.SizeBytes);
            html.Append(Section("Ce qui peut être libéré"))
                .Append("<div class=\"callout\"><div class=\"head\">À retenir</div>")
                .Append(E($"{Formats.Size(safe)} peuvent être libérés sans risque : ces fichiers se recréent seuls ou attendent déjà d'être supprimés. Le reste est à vérifier avant de supprimer."))
                .Append("</div><table><tr><th>Élément</th><th>Avis</th><th>Explication</th><th class=\"size\">Taille</th></tr>");
            foreach (RecoverableLine line in lines)
            {
                html.Append("<tr><td>").Append(E(line.Name)).Append("</td><td>")
                    .Append(line.Safe ? "<span class=\"badge safe\">sans risque</span>" : "<span class=\"badge check\">à vérifier</span>")
                    .Append("</td><td>").Append(E(line.Explanation)).Append("</td><td class=\"size\">").Append(E(line.SizeText)).Append("</td></tr>");
            }

            html.Append("</table></section>");
        }

        // 3. Categories.
        html.Append(Section("Catégories")).Append("<table><tr><th>Catégorie</th><th class=\"size\">Fichiers</th><th class=\"size\">Taille</th></tr>");
        foreach (CategoryTotal category in analysis.Categories.OrderByDescending(c => c.SizeBytes))
        {
            html.Append("<tr><td>").Append(E(ManagerViewModel.CategoryName(category.Category))).Append("</td><td class=\"size\">")
                .Append(category.Files.ToString("N0", French)).Append("</td><td class=\"size\">").Append(E(Formats.Size(category.SizeBytes))).Append("</td></tr>");
        }

        html.Append("</table></section>");

        // 4 and 5. Largest and old files.
        Files(html, Section("Plus gros fichiers"), analysis.Largest.Take(LargestShown).ToList(), analysis.Largest.Count);
        if (analysis.OldFiles.Count > 0)
        {
            Files(html, Section("Fichiers anciens"), analysis.OldFiles.Take(OldShown).ToList(), analysis.OldFiles.Count);
        }

        // Sensitive zones: never by the automatic proposals (D79).
        if (analysis.Largest.Concat(analysis.OldFiles).Any(f => f.Zone == Zone.Sensitive))
        {
            html.Append("<div class=\"callout safety\"><div class=\"head\">Point sécurité — les zones sensibles</div>")
                .Append(E("Les fichiers marqués « zone sensible » appartiennent à Windows ou à des programmes installés. CleanSlate ne les propose jamais lui-même ; les supprimer demande le mode expert et la saisie de « CONFIRMER », et peut empêcher Windows ou un programme de fonctionner."))
                .Append("</div>");
        }

        return html.Append("</body></html>").ToString();
    }

    // Only the characters reserved by HTML: the page is UTF-8, accents stay readable.
    private static string E(string text) => text.Replace("&", "&amp;", StringComparison.Ordinal)
        .Replace("<", "&lt;", StringComparison.Ordinal)
        .Replace(">", "&gt;", StringComparison.Ordinal)
        .Replace("\"", "&quot;", StringComparison.Ordinal);

    private static void Files(StringBuilder html, string section, IReadOnlyList<FileEntry> files, int total)
    {
        html.Append(section);
        if (total > files.Count)
        {
            html.Append("<p>").Append(E(string.Format(French, "Les {0:N0} premiers sur {1:N0}.", files.Count, total))).Append("</p>");
        }

        html.Append("<table><tr><th>Fichier</th><th>Type</th><th>Modifié</th><th class=\"size\">Taille</th></tr>");
        foreach (FileEntry file in files)
        {
            html.Append("<tr><td><code>").Append(E(file.Path)).Append("</code>")
                .Append(file.Zone == Zone.Sensitive ? " <span class=\"badge sensitive\">zone sensible</span>" : string.Empty)
                .Append("</td><td>").Append(E(ManagerViewModel.TypeText(file.Path, file.Category))).Append("</td><td>")
                .Append(E(file.Modified is { } m ? Formats.Date(DateOnly.FromDateTime(m.LocalDateTime)) : Formats.Unknown))
                .Append("</td><td class=\"size\">").Append(E(Formats.Size(file.SizeBytes))).Append("</td></tr>");
        }

        html.Append("</table></section>");
    }
}
