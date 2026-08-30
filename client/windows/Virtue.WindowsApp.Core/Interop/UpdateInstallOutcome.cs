namespace Virtue.WindowsApp.Core.Interop;

/// <summary>
/// Result of an attempt to install a staged Store update. Lives here rather than next to
/// <c>StoreUpdateManager</c> so the callers' decision logic stays free of WinRT — the same
/// constraint that keeps <see cref="UpdateRestartPolicy"/> in this project.
/// </summary>
public enum UpdateInstallOutcome
{
    /// <summary>The install completed (the OS normally terminates the process for the swap).</summary>
    Installed,

    /// <summary>
    /// Nothing was attempted because the install can only proceed through an OS-shown consent
    /// dialog and the caller asked for a silent attempt. The update stays staged; the caller is
    /// expected to put a real window on screen and retry interactively.
    /// </summary>
    NeedsUserInteraction,

    /// <summary>The install was attempted and did not complete.</summary>
    Failed,
}
