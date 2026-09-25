package org.virtueinitiative.virtue

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.os.Build
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.pm.PackageInfoCompat
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.concurrent.TimeUnit

/**
 * Self-update for sideloaded stable builds (issue #716).
 *
 * A worker polls the stable feed ([UpdateManifest]), downloads a newer APK
 * into app storage, checks its SHA-256 against the feed, and checks it is this
 * package with a higher versionCode. Android itself refuses an update signed
 * with a different key.
 *
 * Installing goes through a PackageInstaller session:
 *
 * - Once Virtue is its own installer of record (true after the first in-app
 *   update, never after a manual sideload), Android 12+ installs it silently
 *   with USER_ACTION_NOT_REQUIRED, from the background, and the accessibility
 *   service reconnects in the new process within about a second.
 * - Otherwise (the first update, or Android 10/11) the user has to confirm.
 *   A background app can't open that dialog, so a notification and an
 *   in-app button lead to MainActivity, which starts the session while in the
 *   foreground. If "Install unknown apps" is off for Virtue, Android's
 *   dialog sends the user to that switch first and then carries on.
 */
object AppUpdater {
    private const val TAG = "AppUpdater"
    private const val PERIODIC_WORK = "virtue-update-check"
    private const val ONE_SHOT_WORK = "virtue-update-check-now"
    private const val CHANNEL_ID = "virtue_updates"
    private const val NOTIFICATION_ID = 1002
    private const val PREFS = "app_update"
    private const val KEY_READY_VERSION = "ready_version"
    private const val KEY_READY_SHA256 = "ready_sha256"
    private const val APK_NAME = "update.apk"

    const val ACTION_INSTALL_UPDATE = "org.virtueinitiative.virtue.action.INSTALL_UPDATE"
    internal const val EXTRA_INTERACTIVE = "interactive"

    private val lock = Any()

    val isEnabled: Boolean get() = BuildConfig.AUTO_UPDATE

    /**
     * Set while MainActivity is on screen. A silent install kills the process,
     * so it waits rather than closing the app under the user; the install
     * button covers that case.
     */
    @Volatile
    var uiVisible = false

    fun schedule(context: Context) {
        if (!isEnabled) return
        val request = PeriodicWorkRequestBuilder<UpdateCheckWorker>(6, TimeUnit.HOURS)
            .setConstraints(networkConstraints())
            .build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork(
            PERIODIC_WORK,
            ExistingPeriodicWorkPolicy.KEEP,
            request
        )
    }

    /** Queues a one-off check, e.g. when the app is opened. */
    fun checkSoon(context: Context) {
        if (!isEnabled) return
        val request = OneTimeWorkRequestBuilder<UpdateCheckWorker>()
            .setConstraints(networkConstraints())
            .build()
        WorkManager.getInstance(context).enqueueUniqueWork(
            ONE_SHOT_WORK,
            ExistingWorkPolicy.KEEP,
            request
        )
    }

    // The APK is over 100 MB, so only fetch it on Wi-Fi or another unmetered network.
    private fun networkConstraints() = Constraints.Builder()
        .setRequiredNetworkType(NetworkType.UNMETERED)
        .build()

    /** The version of a downloaded, verified update waiting for the user, if any. */
    fun readyVersion(context: Context): String? {
        if (!isEnabled) return null
        val version = prefs(context).getString(KEY_READY_VERSION, null) ?: return null
        return version.takeIf { apkFile(context).exists() }
    }

    /** Checks the feed and downloads, then installs or announces, any newer release. */
    fun check(context: Context) {
        if (!isEnabled) return
        synchronized(lock) { checkLocked(context) }
    }

    private fun checkLocked(context: Context) {
        val manifest = fetchManifest() ?: return
        if (!UpdateManifest.isNewer(manifest.version, BuildConfig.VIRTUE_BASE_VERSION)) {
            Log.i(TAG, "Up to date (${BuildConfig.VIRTUE_BASE_VERSION}, feed ${manifest.version})")
            clear(context)
            return
        }

        val apk = apkFile(context)
        val prefs = prefs(context)
        val alreadyDownloaded = prefs.getString(KEY_READY_VERSION, null) == manifest.version &&
            prefs.getString(KEY_READY_SHA256, null) == manifest.sha256 &&
            apk.exists()
        if (!alreadyDownloaded) {
            clear(context)
            if (!download(manifest, apk)) return
            if (!isInstallableUpdate(context, apk)) {
                apk.delete()
                return
            }
            prefs.edit()
                .putString(KEY_READY_VERSION, manifest.version)
                .putString(KEY_READY_SHA256, manifest.sha256)
                .apply()
        }

        if (canInstallSilently(context)) {
            if (uiVisible) {
                Log.i(TAG, "Update ${manifest.version} waits until the app is closed")
            } else {
                Log.i(TAG, "Installing ${manifest.version} silently")
                commit(context, apk, interactive = false)
            }
        } else {
            Log.i(TAG, "Update ${manifest.version} needs the user to confirm")
            notifyUpdateReady(context)
        }
    }

    /**
     * Starts an install the user confirms. Call from a visible activity:
     * Android only lets a foreground app open the confirmation dialog.
     * Returns false if there is no verified update to install.
     */
    fun installReadyUpdate(context: Context): Boolean {
        synchronized(lock) { return installReadyLocked(context) }
    }

    private fun installReadyLocked(context: Context): Boolean {
        val apk = apkFile(context)
        val expected = prefs(context).getString(KEY_READY_SHA256, null)
        if (readyVersion(context) == null || expected == null || sha256(apk) != expected) {
            clear(context)
            return false
        }
        commit(context, apk, interactive = true)
        return true
    }

    private fun fetchManifest(): UpdateManifest? = runCatching {
        val connection = URL(BuildConfig.UPDATE_MANIFEST_URL).openConnection() as HttpURLConnection
        connection.connectTimeout = 30_000
        connection.readTimeout = 30_000
        try {
            if (connection.responseCode != 200) {
                Log.w(TAG, "Update feed returned ${connection.responseCode}")
                return null
            }
            val body = connection.inputStream.bufferedReader().use { it.readText() }
            UpdateManifest.parse(body, allowInsecureUrl = BuildConfig.DEBUG).also {
                if (it == null) Log.i(TAG, "Update feed offers no update")
            }
        } finally {
            connection.disconnect()
        }
    }.onFailure { Log.w(TAG, "Update feed fetch failed", it) }.getOrNull()

    private fun download(manifest: UpdateManifest, dest: File): Boolean {
        val part = File(dest.path + ".part")
        return runCatching {
            dest.parentFile?.mkdirs()
            val connection = URL(manifest.url).openConnection() as HttpURLConnection
            connection.connectTimeout = 30_000
            connection.readTimeout = 60_000
            try {
                if (connection.responseCode != 200) {
                    Log.w(TAG, "APK download returned ${connection.responseCode}")
                    return false
                }
                val digest = MessageDigest.getInstance("SHA-256")
                connection.inputStream.use { input ->
                    part.outputStream().use { output ->
                        val buffer = ByteArray(64 * 1024)
                        while (true) {
                            val read = input.read(buffer)
                            if (read < 0) break
                            digest.update(buffer, 0, read)
                            output.write(buffer, 0, read)
                        }
                    }
                }
                val actual = digest.digest().toHex()
                if (actual != manifest.sha256) {
                    Log.w(TAG, "APK SHA-256 mismatch: expected ${manifest.sha256}, got $actual")
                    part.delete()
                    return false
                }
                part.renameTo(dest)
            } finally {
                connection.disconnect()
            }
        }.onFailure {
            Log.w(TAG, "APK download failed", it)
            part.delete()
        }.getOrDefault(false)
    }

    private fun isInstallableUpdate(context: Context, apk: File): Boolean {
        val pm = context.packageManager
        val archive = pm.getPackageArchiveInfo(apk.path, 0)
        val installed = pm.getPackageInfo(context.packageName, 0)
        if (archive == null || archive.packageName != context.packageName) {
            Log.w(TAG, "Downloaded APK is not ${context.packageName}")
            return false
        }
        val newCode = PackageInfoCompat.getLongVersionCode(archive)
        val oldCode = PackageInfoCompat.getLongVersionCode(installed)
        if (newCode <= oldCode) {
            Log.w(TAG, "Downloaded APK versionCode $newCode is not above installed $oldCode")
            return false
        }
        return true
    }

    /**
     * USER_ACTION_NOT_REQUIRED is only honored on Android 12+ when the updating
     * app is the installer of record, and only once "Install unknown apps" is on.
     */
    private fun canInstallSilently(context: Context): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return false
        val pm = context.packageManager
        if (!pm.canRequestPackageInstalls()) return false
        val installer = runCatching {
            pm.getInstallSourceInfo(context.packageName).installingPackageName
        }.getOrNull()
        return installer == context.packageName
    }

    private fun commit(context: Context, apk: File, interactive: Boolean) {
        val installer = context.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
        params.setAppPackageName(context.packageName)
        params.setSize(apk.length())
        if (!interactive && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
        }
        val sessionId = installer.createSession(params)
        try {
            installer.openSession(sessionId).use { session ->
                session.openWrite("base.apk", 0, apk.length()).use { out ->
                    apk.inputStream().use { it.copyTo(out) }
                    session.fsync(out)
                }
                val status = Intent(context, UpdateInstallReceiver::class.java)
                    .putExtra(EXTRA_INTERACTIVE, interactive)
                // Mutable: the installer fills in the status extras.
                val pending = PendingIntent.getBroadcast(
                    context,
                    sessionId,
                    status,
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE
                )
                session.commit(pending.intentSender)
            }
        } catch (t: Throwable) {
            Log.w(TAG, "Install session failed", t)
            runCatching { installer.abandonSession(sessionId) }
        }
    }

    internal fun notifyUpdateReady(context: Context) {
        val version = readyVersion(context) ?: return
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_ID,
                context.getString(R.string.update_channel_name),
                NotificationManager.IMPORTANCE_DEFAULT
            )
        )
        val open = Intent(context, MainActivity::class.java)
            .setAction(ACTION_INSTALL_UPDATE)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        val pending = PendingIntent.getActivity(
            context,
            0,
            open,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val notification = NotificationCompat.Builder(context, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_download_done)
            .setContentTitle(context.getString(R.string.update_notification_title))
            .setContentText(context.getString(R.string.update_notification_text, version))
            .setContentIntent(pending)
            .setAutoCancel(true)
            .setOnlyAlertOnce(true)
            .build()
        manager.notify(NOTIFICATION_ID, notification)
    }

    /** Drops any downloaded update and its notification. */
    internal fun clear(context: Context) {
        prefs(context).edit().remove(KEY_READY_VERSION).remove(KEY_READY_SHA256).apply()
        apkFile(context).parentFile?.listFiles()?.forEach { it.delete() }
        context.getSystemService(NotificationManager::class.java).cancel(NOTIFICATION_ID)
    }

    private fun apkFile(context: Context) = File(File(context.noBackupFilesDir, "updates"), APK_NAME)

    private fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    private fun sha256(file: File): String? = runCatching {
        val digest = MessageDigest.getInstance("SHA-256")
        file.inputStream().use { input ->
            val buffer = ByteArray(64 * 1024)
            while (true) {
                val read = input.read(buffer)
                if (read < 0) break
                digest.update(buffer, 0, read)
            }
        }
        digest.digest().toHex()
    }.getOrNull()

    private fun ByteArray.toHex() = joinToString("") { "%02x".format(it) }
}

class UpdateCheckWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        AppUpdater.check(applicationContext)
        // Failures are retried by the next periodic run rather than by backoff.
        return Result.success()
    }
}

/** Receives PackageInstaller session results for [AppUpdater]. */
class UpdateInstallReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)
        val interactive = intent.getBooleanExtra(AppUpdater.EXTRA_INTERACTIVE, false)
        when (status) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                if (interactive) {
                    @Suppress("DEPRECATION")
                    val confirm = intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT) ?: return
                    context.startActivity(confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                } else {
                    // Android wanted confirmation after all. Drop this session
                    // and let the user start one from the foreground.
                    val sessionId = intent.getIntExtra(PackageInstaller.EXTRA_SESSION_ID, -1)
                    runCatching { context.packageManager.packageInstaller.abandonSession(sessionId) }
                    AppUpdater.notifyUpdateReady(context)
                }
            }
            // Delivered to the new process after it replaces this one.
            PackageInstaller.STATUS_SUCCESS -> {
                Log.i("AppUpdater", "Update installed")
                AppUpdater.clear(context)
            }
            else -> Log.w(
                "AppUpdater",
                "Update install failed ($status): ${intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE)}"
            )
        }
    }
}
