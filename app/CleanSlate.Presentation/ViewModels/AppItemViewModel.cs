using CleanSlate.Interop;

namespace CleanSlate.Presentation.ViewModels;

/// <summary>One row of the Applications list: the app plus its display texts.</summary>
public sealed class AppItemViewModel
{
    /// <summary>Wraps an installed application.</summary>
    public AppItemViewModel(InstalledApp app)
    {
        App = app;
        SizeText = Formats.SizeFromKb(app.EstimatedSizeKb);
        DateText = Formats.Date(app.InstallDate);
        Initials = MakeInitials(app.DisplayName);
        AutomationName = string.Join(", ", new[] { app.DisplayName, app.Publisher, app.Version, SizeText }
            .Where(s => !string.IsNullOrWhiteSpace(s) && s != Formats.Unknown));
    }

    /// <summary>Underlying engine record.</summary>
    public InstalledApp App { get; }

    /// <summary>Name.</summary>
    public string DisplayName => App.DisplayName;

    /// <summary>Publisher, or « — ».</summary>
    public string PublisherText => App.Publisher ?? Formats.Unknown;

    /// <summary>Version, or « — ».</summary>
    public string VersionText => App.Version ?? Formats.Unknown;

    /// <summary>Size, e.g. « 412 Mo », or « — ».</summary>
    public string SizeText { get; }

    /// <summary>Installation date, e.g. « 18 juin 2025 », or « — ».</summary>
    public string DateText { get; }

    /// <summary>Two-letter monogram, e.g. « Gi » for GIMP.</summary>
    public string Initials { get; }

    /// <summary>Install folder, or « — ».</summary>
    public string LocationText => App.InstallLocation ?? Formats.Unknown;

    /// <summary>Installer technology name, empty when unknown (proper nouns, not translated).</summary>
    public string InstallerName => App.Installer switch
    {
        InstallerKind.Msi => "Windows Installer (MSI)",
        InstallerKind.InnoSetup => "Inno Setup",
        InstallerKind.Nsis => "NSIS",
        InstallerKind.Msix => "MSIX (Microsoft Store)",
        _ => string.Empty,
    };

    /// <summary>True when the installer is known.</summary>
    public bool HasInstallerName => InstallerName.Length > 0;

    /// <summary>Microsoft Store application (badge « Store », EF-04).</summary>
    public bool IsStore => App.Kind == AppKind.Store;

    /// <summary>Store package that CleanSlate never removes: Microsoft's or preinstalled (D52).</summary>
    public bool IsProtectedStore => IsStore && App.HiddenReason is not null;

    /// <summary>Why it is protected, turned into text by the view (<c>HiddenReason_{value}</c>).</summary>
    public string HiddenReasonKey => App.HiddenReason?.ToString() ?? string.Empty;

    /// <summary>True for system components and updates (EF-02).</summary>
    public bool IsHidden => App.HiddenReason is not null;

    /// <summary>Text read by Narrator for the row.</summary>
    public string AutomationName { get; }

    private static string MakeInitials(string name)
    {
        char[] letters = name.Where(c => char.IsLetterOrDigit(c) || c == '+').Take(2).ToArray();
        return letters.Length switch
        {
            0 => "?",
            1 => char.ToUpperInvariant(letters[0]).ToString(),
            _ => string.Concat(char.ToUpperInvariant(letters[0]), char.ToLowerInvariant(letters[1])),
        };
    }
}
