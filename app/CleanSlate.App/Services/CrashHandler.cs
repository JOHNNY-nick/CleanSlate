using System.Runtime.InteropServices;
using CleanSlate.Presentation;
using Microsoft.Windows.ApplicationModel.Resources;

namespace CleanSlate.App.Services;

/// <summary>
/// Last line of defence (D56): an unexpected exception never closes CleanSlate without a word.
/// It is written in full to the crash log (CrashLog) and a clear message is shown.
/// </summary>
internal static class CrashHandler
{
    private const uint MbOk = 0x0, MbIconError = 0x10, MbTaskModal = 0x2000, MbSetForeground = 0x10000;
    private static readonly object Gate = new();
    private static DateTime _lastMessage = DateTime.MinValue;

    /// <summary>Exception of the UI thread: logged, shown, and CleanSlate keeps running.</summary>
    public static void OnUiException(Microsoft.UI.Xaml.UnhandledExceptionEventArgs e)
    {
        e.Handled = true;
        string? file = CrashLog.Write(CrashLog.CurrentDirectory(), e.Exception, "Application.UnhandledException", DateTimeOffset.Now, e.Message);
        Show("Crash_Message", file, throttle: true);
    }

    /// <summary>Exception of another thread: the process ends; logged and shown first.</summary>
    public static void OnDomainException(UnhandledExceptionEventArgs e)
    {
        Exception exception = e.ExceptionObject as Exception ?? new InvalidOperationException(e.ExceptionObject?.ToString());
        string? file = CrashLog.Write(CrashLog.CurrentDirectory(), exception, "AppDomain.UnhandledException", DateTimeOffset.Now, $"IsTerminating = {e.IsTerminating}");
        if (e.IsTerminating)
        {
            Show("Crash_MessageFatal", file, throttle: false);
        }
    }

    /// <summary>Faulted task nobody awaited: logged only, it does not stop CleanSlate.</summary>
    public static void OnUnobservedTask(UnobservedTaskExceptionEventArgs e)
    {
        CrashLog.Write(CrashLog.CurrentDirectory(), e.Exception, "TaskScheduler.UnobservedTaskException", DateTimeOffset.Now);
        e.SetObserved();
    }

    // A native message box: it works even when the XAML tree is in a bad state, unlike a
    // ContentDialog. At most one message every 5 s, so that a failing layout cannot flood.
    private static void Show(string messageKey, string? file, bool throttle)
    {
        lock (Gate)
        {
            if (throttle && DateTime.UtcNow - _lastMessage < TimeSpan.FromSeconds(5))
            {
                return;
            }

            _lastMessage = DateTime.UtcNow;
        }

        try
        {
            var loader = new ResourceLoader();
            string where = file ?? CrashLog.CurrentDirectory();
            MessageBoxW(IntPtr.Zero, string.Format(loader.GetString(messageKey), where), loader.GetString("Crash_Title"), MbOk | MbIconError | MbTaskModal | MbSetForeground);
        }
        catch (Exception)
        {
            // Nothing more can be done: the entry is already in the crash log.
        }
    }

    // DllImport rather than LibraryImport: the latter needs unsafe code in the application,
    // and pointers stay in CleanSlate.Interop only.
#pragma warning disable SYSLIB1054
    [DllImport("user32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern int MessageBoxW(IntPtr owner, string text, string caption, uint type);
#pragma warning restore SYSLIB1054
}
