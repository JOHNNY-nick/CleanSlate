using CleanSlate.App.Services;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace CleanSlate.App.ViewModels;

/// <summary>Dashboard. In J1 it only shows the engine version read through the DLL.</summary>
public sealed partial class HomeViewModel : ObservableObject
{
    private readonly IEngineService _engine;

    /// <summary>Creates the ViewModel.</summary>
    public HomeViewModel(IEngineService engine)
    {
        _engine = engine;
    }

    /// <summary>Engine version, e.g. <c>0.1.0</c>; empty until loaded.</summary>
    [ObservableProperty]
    private string _engineVersion = string.Empty;

    /// <summary>True when the engine could not be reached.</summary>
    [ObservableProperty]
    private bool _hasError;

    /// <summary>Technical details of the last error, shown under the message.</summary>
    [ObservableProperty]
    private string _errorDetails = string.Empty;

    /// <summary>Reads the engine version.</summary>
    [RelayCommand]
    private async Task LoadAsync(CancellationToken cancellationToken)
    {
        try
        {
            EngineVersion = await _engine.GetVersionAsync(cancellationToken);
            HasError = false;
            ErrorDetails = string.Empty;
        }
        catch (Exception ex) when (ex is not OperationCanceledException)
        {
            HasError = true;
            ErrorDetails = ex.Message;
        }
    }
}
