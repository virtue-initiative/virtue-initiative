namespace Virtue.WindowsApp.Core.Interop;

/// <summary>
/// The one folder that holds everything the app writes (CLIENT-002): the native layer's config,
/// state and logs, <c>ui-startup.log</c> and the small flag files.
///
/// <para>
/// A packaged install keeps it in the package's own <c>LocalState\Virtue</c>, which Windows
/// deletes when the package is uninstalled. That is what makes uninstall clean (CLIENT-014): an
/// MSIX uninstall runs no app code, so anything kept outside the package, as
/// <c>%ProgramData%\Virtue</c> was, would need something outside the package to delete it.
/// </para>
///
/// <para>
/// The app picks the folder once, in its constructor, with <see cref="Initialize"/>, which also
/// hands it to the native layer through <see cref="EnvironmentVariable"/>. Unpackaged runs (dev,
/// tests) keep <c>%ProgramData%\Virtue</c>.
/// </para>
/// </summary>
public static class AppDataDirectory
{
    /// <summary>Must match <c>BASE_DIR_ENV</c> in <c>client/windows/src/config.rs</c>.</summary>
    public const string EnvironmentVariable = "VIRTUE_WINDOWS_BASE_DIR";

    private const string FolderName = "Virtue";

    /// <summary>Where every version before this one kept its state, shared by all Windows users.</summary>
    public static string LegacyPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
        FolderName);

    /// <summary>The folder chosen by <see cref="Initialize"/>; <see cref="LegacyPath"/> until then.</summary>
    public static string Current { get; private set; } = LegacyPath;

    /// <summary>
    /// Chooses the folder for this run and tells the native layer. Call it before anything
    /// reads or writes app files.
    /// </summary>
    /// <param name="packageLocalStatePath">
    /// <c>ApplicationData.Current.LocalFolder.Path</c>, or <c>null</c> for an unpackaged run.
    /// </param>
    /// <returns>Lines for the startup log.</returns>
    public static IReadOnlyList<string> Initialize(string? packageLocalStatePath)
    {
        var log = new List<string>();
        Current = string.IsNullOrWhiteSpace(packageLocalStatePath)
            ? LegacyPath
            : Prepare(LegacyPath, Path.Combine(packageLocalStatePath, FolderName), log.Add);
        Environment.SetEnvironmentVariable(EnvironmentVariable, Current);
        log.Add($"App data directory: {Current}");
        return log;
    }

    /// <summary>
    /// Returns the folder to use for a packaged run, first moving an older version's
    /// <paramref name="legacyDir"/> into <paramref name="packageDir"/> when only the former
    /// exists (CLIENT-002).
    ///
    /// The move copies into a temporary sibling and renames it into place, so
    /// <paramref name="packageDir"/> only ever appears complete: a half-finished copy can't be
    /// mistaken for a finished one by the next launch. The copy, rather than a move, also gives
    /// the files the package folder's per-user permissions instead of carrying over
    /// ProgramData's. If anything fails before the rename, this run keeps using
    /// <paramref name="legacyDir"/> and the next launch tries again, so no recorded data is
    /// lost.
    /// </summary>
    public static string Prepare(string legacyDir, string packageDir, Action<string> log)
    {
        if (Directory.Exists(packageDir) || !Directory.Exists(legacyDir))
        {
            return packageDir;
        }

        var parent = Path.GetDirectoryName(packageDir)!;
        var staging = Path.Combine(parent, $"{Path.GetFileName(packageDir)}.moving-{Guid.NewGuid():N}");
        try
        {
            CopyDirectory(legacyDir, staging);
            Directory.Move(staging, packageDir);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            TryDelete(staging);
            if (Directory.Exists(packageDir))
            {
                // Another launch finished the move first, and owns deleting the old folder.
                log($"App data was moved by another launch; using {packageDir}.");
                return packageDir;
            }

            log($"Could not move app data from {legacyDir} ({ex.Message}); using it for now.");
            return legacyDir;
        }

        log($"Moved app data from {legacyDir} to {packageDir}.");
        if (!TryDelete(legacyDir))
        {
            log($"Could not delete the old app data in {legacyDir}.");
        }

        return packageDir;
    }

    private static void CopyDirectory(string source, string destination)
    {
        Directory.CreateDirectory(destination);
        foreach (var file in Directory.EnumerateFiles(source))
        {
            File.Copy(file, Path.Combine(destination, Path.GetFileName(file)));
        }

        foreach (var directory in Directory.EnumerateDirectories(source))
        {
            CopyDirectory(directory, Path.Combine(destination, Path.GetFileName(directory)));
        }
    }

    private static bool TryDelete(string directory)
    {
        try
        {
            if (Directory.Exists(directory))
            {
                Directory.Delete(directory, recursive: true);
            }

            return true;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return false;
        }
    }
}
