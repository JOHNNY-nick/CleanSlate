using CleanSlate.App.Services;
using CleanSlate.App.ViewModels;
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

        _host = Host.CreateDefaultBuilder()
            .ConfigureServices(services =>
            {
                services.AddSingleton<IEngineService, EngineService>();
                services.AddSingleton<MainWindow>();
                services.AddTransient<HomeViewModel>();
                services.AddTransient<AppsViewModel>();
                services.AddTransient<OrphansViewModel>();
                services.AddTransient<HistoryViewModel>();
                services.AddTransient<SettingsViewModel>();
            })
            .Build();
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
        _window = GetService<MainWindow>();
        _window.Closed += (_, _) => _host.Dispose();
        _window.Activate();
    }
}
