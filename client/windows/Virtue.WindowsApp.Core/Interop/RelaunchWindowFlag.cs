namespace Virtue.WindowsApp.Core.Interop;

/// <summary>
/// One-shot marker asking the <i>next</i> launch to come back with the main window on screen
/// instead of quietly into the tray.
///
/// Installing a Store update means the OS terminates the process and
/// <see cref="RestartWatchdog"/> relaunches it (quietly, within a minute). That is right for an
/// automatic background update, but when the user pressed "Restart now to update" themselves the
/// app appearing to simply vanish is the wrong feedback — they asked for a restart, so they
/// should see it come back. The flag is set just before the install and consumed on the next
/// launch; it deliberately survives the process termination, since that termination is the whole
/// point.
/// </summary>
public static class RelaunchWindowFlag
{
    public static string FlagPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
        "Virtue",
        "show-window-on-next-launch");

    /// <summary>Best-effort: failing to set the flag only costs the window restore.</summary>
    public static void Set(string path)
    {
        try
        {
            var directory = Path.GetDirectoryName(path);
            if (!string.IsNullOrWhiteSpace(directory))
            {
                Directory.CreateDirectory(directory);
            }

            File.WriteAllText(path, string.Empty);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
        }
    }

    /// <summary>
    /// Deletes the flag and reports whether it was set. Always call it, even when the window is
    /// being shown anyway, so a flag left behind by an install that never completed doesn't pop
    /// a window at some unrelated later launch.
    /// </summary>
    public static bool TryConsume(string path)
    {
        try
        {
            if (!File.Exists(path))
            {
                return false;
            }

            File.Delete(path);
            return true;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return false;
        }
    }
}
