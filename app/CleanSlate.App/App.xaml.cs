using CleanSlate.App.Services;
using CleanSlate.Presentation.Services;
using CleanSlate.Presentation.ViewModels;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.UI.Xaml;

namespace CleanSlate.App;

/// <summary>Application entry point: builds the DI container and opens the main window.</summary>
public partial class App : Application
{
    private readonly IHost _host;
    private Window? _window;

    /// <summary>Configures the generic host and its services.</summary>
    public App()
    {
        InitializeComponent();

        // Never close without a word (D56): log the full exception and show a message.
        UnhandledException += (_, e) => CrashHandler.OnUiException(e);
        AppDomain.CurrentDomain.UnhandledException += (_, e) => CrashHandler.OnDomainException(e);
        TaskScheduler.UnobservedTaskException += (_, e) => CrashHandler.OnUnobservedTask(e);

        _host = Host.CreateDefaultBuilder()
            .ConfigureServices(services =>
            {
                services.AddSingleton<ISettingsService, SettingsService>();
                services.AddSingleton<IEngineService, EngineService>();
                services.AddSingleton<MainWindow>();
                services.AddSingleton<IDialogService, DialogService>();
        services.AddSingleton<ILogExportService, LogExportService>();
        services.AddSingleton<IFolderService, FolderService>();
        services.AddTransient<OnboardingViewModel>();
                services.AddSingleton<INavigationService, NavigationService>();
                services.AddSingleton<UninstallWizardViewModel>();
                services.AddTransient(sp => new HomeViewModel(
                    sp.GetRequiredService<IEngineService>(),
                    sp.GetRequiredService<ISettingsService>(),
                    sp.GetRequiredService<INavigationService>(),
                    sp.GetRequiredService<OrphansViewModel>(),
                    IsElevated()));
                services.AddTransient<AppsViewModel>();
                // Singleton: scan results survive navigation between pages.
                services.AddSingleton<OrphansViewModel>();
                services.AddTransient<HistoryViewModel>();
                services.AddTransient<SettingsViewModel>();
            })
            .Build();
    }

    /// <summary>True when the process runs with administrator rights.</summary>
    public static bool IsElevated()
    {
        using var identity = System.Security.Principal.WindowsIdentity.GetCurrent();
        return new System.Security.Principal.WindowsPrincipal(identity)
            .IsInRole(System.Security.Principal.WindowsBuiltInRole.Administrator);
    }

    /// <summary>Restarts CleanSlate with administrator rights (UAC prompt), then exits.</summary>
    public static void RestartElevated()
    {
        try
        {
            System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(Environment.ProcessPath!)
            {
                UseShellExecute = true,
                Verb = "runas",
            });
            Current.Exit();
        }
        catch (System.ComponentModel.Win32Exception)
        {
            // The user refused the UAC prompt: keep running.
        }
    }

    /// <summary>Resolves a registered service (used by pages to get their ViewModel).</summary>
    public static T GetService<T>()
        where T : class
    {
        if ((Current as App)?._host.Services.GetService(typeof(T)) is not T service)
        {
            throw new InvalidOperationException($"{typeof(T).Name} is not registered.");
        }

        return service;
    }

    /// <inheritdoc/>
    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        var window = GetService<MainWindow>();
        _window = window;
        var settings = GetService<ISettingsService>();
        window.ApplyTheme(settings.Theme);
        settings.Changed += (_, _) => window.DispatcherQueue.TryEnqueue(() => window.ApplyTheme(settings.Theme));
        window.Closed += (_, _) => _host.Dispose();
        window.Activate();
        _ = StartAsync(window);
    }

    /// <summary>
    /// First launch: the welcome screens and basic settings (D63), then the application; the
    /// automatic purge comes last, with the delay and folder chosen.
    /// </summary>
    private static async Task StartAsync(MainWindow window)
    {
        if (!GetService<ISettingsService>().FirstRunDone)
        {
            await window.ShowOnboardingAsync();
        }

        window.ShowHome();
        await PurgeOldSessionsAsync();
    }

    /// <summary>Automatic purge at startup (EF-19); does nothing in simulation mode.</summary>
    private static async Task PurgeOldSessionsAsync()
    {
        var settings = GetService<ISettingsService>();
        if (settings.SimulationMode)
        {
            return;
        }

        try
        {
            var request = new Interop.PurgeRequest((uint)settings.QuarantineDays, DryRun: false);
            await GetService<IEngineService>().PurgeAsync(request, CancellationToken.None);
        }
        catch (Interop.EngineException)
        {
            // A failed purge is retried at the next start; nothing to show.
        }
    }
}
