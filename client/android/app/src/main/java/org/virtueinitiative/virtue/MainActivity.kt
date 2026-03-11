package org.virtueinitiative.virtue

import android.Manifest
import android.content.Context
import android.content.Intent
import android.media.projection.MediaProjectionManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.lifecycle.lifecycleScope
import org.virtueinitiative.virtue.databinding.ActivityMainBinding
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MainActivity : AppCompatActivity() {
    private lateinit var binding: ActivityMainBinding

    private val projectionManager by lazy {
        getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
    }

    private val projectionPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.StartActivityForResult()
    ) { result ->
        if (result.resultCode == RESULT_OK && result.data != null) {
            val data = result.data!!
            val persisted = ProjectionPermissionStore.save(this, result.resultCode, data)
            val error = ScreenshotService.start(this, result.resultCode, data)
            if (error == null) {
                if (persisted) {
                    setStatus("Capture permission granted. Monitoring started.")
                } else {
                    setStatus("Capture granted for this app session. Monitoring started.")
                }
            } else {
                setStatus("Capture granted, but service failed to start: $error")
            }
        } else {
            setStatus("Capture permission not granted.")
        }
    }

    private val notificationPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)
        binding.versionText.text = "Build ${BuildConfig.VIRTUE_BUILD_LABEL}"

        populateOverrideInputs()

        val initError = NativeBridge.ensureInitialized(this)
        if (initError != null) {
            setStatus("Core init failed: $initError")
        }

        binding.saveOverridesButton.setOnClickListener { saveOverrides(applyNow = true, showSavedMessage = true) }
        binding.loginButton.setOnClickListener { login() }
        binding.signOutButton.setOnClickListener { logout() }
        binding.grantCaptureButton.setOnClickListener { requestCapturePermission() }
        binding.startServiceButton.setOnClickListener {
            if (ProjectionPermissionStore.load(this) == null) {
                setStatus("Requesting screenshot permission...")
                requestCapturePermission()
                return@setOnClickListener
            }
            val error = ScreenshotService.startFromStoredProjection(this, "manual")
            if (error == null) {
                setStatus("Requested background monitoring start")
            } else {
                setStatus("Could not start monitoring: $error")
            }
        }

        KeepAliveWorker.schedule(this)
        requestBackgroundFriendlySettings()
        refreshUi()
    }

    private fun login() {
        if (!saveOverrides(applyNow = true, showSavedMessage = false)) {
            return
        }

        val email = binding.emailInput.text?.toString()?.trim().orEmpty()
        val password = binding.passwordInput.text?.toString().orEmpty()

        if (email.isBlank() || password.isBlank()) {
            setStatus("Email and password are required")
            return
        }

        binding.loginButton.isEnabled = false
        lifecycleScope.launch {
            val error = withContext(Dispatchers.IO) {
                NativeBridge.nativeLogin(email, password, deviceName())
            }
            binding.loginButton.isEnabled = true

            if (error == null) {
                setStatus("Signed in. Request screenshot permission next.")
                refreshUi()
                requestCapturePermission()
            } else {
                setStatus("Login failed: $error")
            }
        }
    }

    private fun logout() {
        lifecycleScope.launch {
            val error = withContext(Dispatchers.IO) {
                NativeBridge.nativeLogout()
            }

            ScreenshotService.stop(this@MainActivity)
            ProjectionPermissionStore.clear(this@MainActivity)

            if (error == null) {
                setStatus("Signed out")
            } else {
                setStatus("Sign out warning: $error")
            }
            refreshUi()
        }
    }

    private fun requestCapturePermission() {
        if (!NativeBridge.nativeIsLoggedIn()) {
            setStatus("Sign in first")
            return
        }

        projectionPermissionLauncher.launch(projectionManager.createScreenCaptureIntent())
    }

    private fun refreshUi() {
        val loggedIn = NativeBridge.nativeIsLoggedIn()
        binding.loginPanel.visibility = if (loggedIn) android.view.View.GONE else android.view.View.VISIBLE
        binding.sessionPanel.visibility = if (loggedIn) android.view.View.VISIBLE else android.view.View.GONE

        if (loggedIn) {
            val deviceId = NativeBridge.nativeGetDeviceId() ?: "<pending>"
            setStatus("Signed in. Device id: $deviceId")
        }
    }

    private fun requestBackgroundFriendlySettings() {
        if (Build.VERSION.SDK_INT >= 33) {
            notificationPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            val pm = getSystemService(PowerManager::class.java)
            if (!pm.isIgnoringBatteryOptimizations(packageName)) {
                runCatching {
                    val intent = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS)
                        .setData(Uri.parse("package:$packageName"))
                    startActivity(intent)
                }
            }
        }
    }

    private fun setStatus(message: String) {
        binding.statusText.text = message
    }

    private fun deviceName(): String {
        val manufacturer = Build.MANUFACTURER.orEmpty()
        val model = Build.MODEL.orEmpty()
        return if (model.startsWith(manufacturer, ignoreCase = true)) model else "$manufacturer $model"
    }

    private fun populateOverrideInputs() {
        val overrides = OverrideSettings.load(this)
        binding.baseApiUrlInput.setText(overrides.baseApiUrl.orEmpty())
        binding.captureIntervalInput.setText(overrides.captureIntervalSeconds.orEmpty())
        binding.batchWindowInput.setText(overrides.batchWindowSeconds.orEmpty())
    }

    private fun saveOverrides(applyNow: Boolean, showSavedMessage: Boolean): Boolean {
        val baseUrl = binding.baseApiUrlInput.text?.toString().orEmpty().trim()
        val captureInterval = binding.captureIntervalInput.text?.toString().orEmpty().trim()
        val batchWindow = binding.batchWindowInput.text?.toString().orEmpty().trim()

        if (baseUrl.isNotEmpty() && !baseUrl.startsWith("http://") && !baseUrl.startsWith("https://")) {
            setStatus("VIRTUE_BASE_API_URL must start with http:// or https://")
            return false
        }
        if (captureInterval.isNotEmpty() && captureInterval.toLongOrNull()?.let { it > 0 } != true) {
            setStatus("VIRTUE_CAPTURE_INTERVAL_SECONDS must be a positive integer")
            return false
        }
        if (batchWindow.isNotEmpty() && batchWindow.toLongOrNull()?.let { it > 0 } != true) {
            setStatus("VIRTUE_BATCH_WINDOW_SECONDS must be a positive integer")
            return false
        }

        val values = OverrideValues(
            baseApiUrl = baseUrl.ifEmpty { null },
            captureIntervalSeconds = captureInterval.ifEmpty { null },
            batchWindowSeconds = batchWindow.ifEmpty { null }
        )
        OverrideSettings.save(this, values)

        if (applyNow) {
            val error = NativeBridge.applyOverrides(this)
            if (error != null) {
                setStatus("Failed to apply overrides: $error")
                return false
            }
        }

        if (showSavedMessage) {
            setStatus("Overrides saved")
        }

        return true
    }
}
