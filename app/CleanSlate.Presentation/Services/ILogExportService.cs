namespace CleanSlate.Presentation.Services;

/// <summary>Export of the logs and settings for a problem report (implemented with <see cref="LogExport"/>).</summary>
public interface ILogExportService
{
    /// <summary>Writes the zip and returns its path.</summary>
    Task<string> ExportAsync(CancellationToken cancellationToken);

    /// <summary>Shows the zip in Explorer.</summary>
    void Reveal(string path);
}
