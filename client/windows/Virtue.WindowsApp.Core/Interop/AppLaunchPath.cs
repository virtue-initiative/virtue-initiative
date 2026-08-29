namespace Virtue.WindowsApp.Core.Interop;

/// <summary>
/// Resolves a launch path for the resident app that survives a package update.
///
/// <para>
/// <see cref="Environment.ProcessPath"/> for an MSIX app is a <i>version-stamped</i> location
/// (<c>C:\Program Files\WindowsApps\…_0.1.1.0_x64__…\Virtue.WindowsApp.exe</c>). Registering the
/// <see cref="RestartWatchdog"/> task with that path works right up until the moment it matters
/// most: a Store update replaces the package directory with a new version-stamped one, the old
/// path stops existing, and every subsequent watchdog run fails with 0x80070002
/// (<c>ERROR_FILE_NOT_FOUND</c>) — so the app that the update just terminated is never brought
/// back. The task <i>is</i> re-registered on every launch, which repairs the path, but only a
/// launch can do that and nothing is left to launch it.
/// </para>
///
/// <para>
/// The app execution alias declared in <c>Package.appxmanifest</c> gives us a stable per-user
/// stub (<c>%LOCALAPPDATA%\Microsoft\WindowsApps\virtue-initiative.exe</c>) that Windows
/// repoints at the current package version on every deployment, and — unlike
/// <c>explorer.exe shell:AppsFolder\…</c>, the other version-independent way to start a packaged
/// app — it forwards command-line arguments, which the watchdog needs for its quiet-relaunch
/// flag.
/// </para>
/// </summary>
public static class AppLaunchPath
{
    /// <summary>Must match the <c>uap5:AppExecutionAlias</c> in <c>Package.appxmanifest</c>.</summary>
    public const string ExecutionAliasFileName = "virtue-initiative.exe";

    public static string ExecutionAliasPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "Microsoft",
        "WindowsApps",
        ExecutionAliasFileName);

    /// <summary>
    /// Picks the alias stub when it exists, otherwise falls back to the running executable —
    /// unpackaged/dev runs, and the first launch of an older package whose manifest predates the
    /// alias, have no stub. The fallback is exactly the old behaviour: correct until the next
    /// package update.
    /// </summary>
    /// <param name="fileExists">Injected for testability.</param>
    public static string? Resolve(string? executionAliasPath, string? processPath, Func<string, bool> fileExists)
    {
        if (!string.IsNullOrWhiteSpace(executionAliasPath) && fileExists(executionAliasPath))
        {
            return executionAliasPath;
        }

        return string.IsNullOrWhiteSpace(processPath) ? null : processPath;
    }
}
