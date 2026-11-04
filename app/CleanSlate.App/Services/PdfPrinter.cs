using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.Web.WebView2.Core;

namespace CleanSlate.App.Services;

/// <summary>
/// An HTML page printed to PDF (D85) by the Edge engine of Windows (WebView2, shipped with the
/// Windows App SDK): an invisible view attached to the main window, then the DevTools command
/// <c>Page.printToPDF</c> in A4, backgrounds printed, a footer « title · page / pages ».
/// </summary>
internal static class PdfPrinter
{
    /// <summary>Writes <paramref name="html"/> as a PDF at <paramref name="path"/>.</summary>
    /// <exception cref="IOException">The PDF could not be made (French message).</exception>
    public static async Task PrintAsync(string html, string path, string footer, CancellationToken cancellationToken)
    {
        CoreWebView2Controller? controller = null;
        try
        {
            IntPtr window = WinRT.Interop.WindowNative.GetWindowHandle(App.GetService<MainWindow>());
            string userData = Path.Combine(Path.GetTempPath(), "CleanSlate-WebView2");
            CoreWebView2Environment environment = await CoreWebView2Environment.CreateWithOptionsAsync(string.Empty, userData, new CoreWebView2EnvironmentOptions());
            controller = await environment.CreateCoreWebView2ControllerAsync(CoreWebView2ControllerWindowReference.CreateFromWindowHandle((ulong)window));
            controller.IsVisible = false;
            // The report is static HTML: no script, no context menu, no developer tools.
            controller.CoreWebView2.Settings.IsScriptEnabled = false;
            controller.CoreWebView2.Settings.AreDefaultContextMenusEnabled = false;
            controller.CoreWebView2.Settings.AreDevToolsEnabled = false;
            var loaded = new TaskCompletionSource<bool>();
            controller.CoreWebView2.NavigationCompleted += (_, e) => loaded.TrySetResult(e.IsSuccess);
            controller.CoreWebView2.NavigateToString(html);
            if (!await loaded.Task.WaitAsync(TimeSpan.FromSeconds(30), cancellationToken))
            {
                throw new IOException("la page du rapport ne s'est pas affichée.");
            }

            string text = System.Net.WebUtility.HtmlEncode(footer);
            var request = new JsonObject
            {
                ["printBackground"] = true,
                ["preferCSSPageSize"] = true,
                ["displayHeaderFooter"] = true,
                ["headerTemplate"] = "<span></span>",
                ["footerTemplate"] = "<div style=\"width:100%;padding:0 16mm;display:flex;justify-content:space-between;font-family:'Segoe UI',sans-serif;font-size:8px;color:#8a86a3\">"
                    + $"<span>{text}</span><span><span class=\"pageNumber\"></span> / <span class=\"totalPages\"></span></span></div>",
            };
            string answer = await controller.CoreWebView2.CallDevToolsProtocolMethodAsync("Page.printToPDF", request.ToJsonString());
            using JsonDocument result = JsonDocument.Parse(answer);
            byte[] pdf = Convert.FromBase64String(result.RootElement.GetProperty("data").GetString() ?? string.Empty);
            await File.WriteAllBytesAsync(path, pdf, cancellationToken);
        }
        catch (Exception ex) when (ex is System.Runtime.InteropServices.COMException or JsonException or FormatException or KeyNotFoundException or TimeoutException)
        {
            throw new IOException($"le moteur Edge de Windows (WebView2) n'a pas pu créer le PDF ({ex.Message}).", ex);
        }
        finally
        {
            controller?.Close();
        }
    }
}
