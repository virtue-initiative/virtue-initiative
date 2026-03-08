package org.virtueinitiative.virtue

import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit

class KeepAliveWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val initError = NativeBridge.ensureInitialized(applicationContext)
        if (initError != null) {
            return Result.retry()
        }

        if (!NativeBridge.nativeIsLoggedIn()) {
            return Result.success()
        }
        if (ProjectionPermissionStore.load(applicationContext) == null) {
            return Result.success()
        }

        ScreenshotService.startFromStoredProjection(applicationContext, "worker")
        return Result.success()
    }

    companion object {
        private const val UNIQUE_NAME = "virtue-keepalive"

        fun schedule(context: Context) {
            val request = PeriodicWorkRequestBuilder<KeepAliveWorker>(15, TimeUnit.MINUTES).build()
            WorkManager.getInstance(context).enqueueUniquePeriodicWork(
                UNIQUE_NAME,
                ExistingPeriodicWorkPolicy.UPDATE,
                request
            )
        }
    }
}
