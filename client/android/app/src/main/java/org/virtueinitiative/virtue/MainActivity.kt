package org.virtueinitiative.virtue

import android.Manifest
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import android.app.Dialog
import android.graphics.drawable.GradientDrawable
import android.view.Window
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.lifecycle.lifecycleScope
import org.virtueinitiative.virtue.databinding.ActivityMainBinding
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

class MainActivity : AppCompatActivity() {
    private lateinit var binding: ActivityMainBinding

    private val notificationPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)
        binding.versionText.text = "Build ${BuildConfig.VIRTUE_BUILD_LABEL}"

        populateOverrideInputs()

        if (binding.deviceNameInput.text.isNullOrBlank()) {
            binding.deviceNameInput.setText(deviceName())
        }

        val initError = NativeBridge.ensureInitialized(this)
        if (initError != null) {
            setStatus("Core init failed: $initError")
        }

        binding.saveOverridesButton.setOnClickListener { saveOverrides(applyNow = true, showSavedMessage = true) }
        binding.loginButton.setOnClickListener { login() }
        binding.signOutButton.setOnClickListener { logout() }
        binding.overridesButton.setOnClickListener { showOverridesDialog() }
        binding.overridesButtonSession.setOnClickListener { showOverridesDialog() }
        binding.statusDetailsButton.setOnClickListener { showStatusDetails() }
        binding.pauseResumeButton.setOnClickListener { toggleMonitoring() }
        binding.grantCaptureButton.setOnClickListener { openAccessibilitySettings() }
        binding.startServiceButton.setOnClickListener { openAccessibilitySettings() }

        binding.websiteLink.setOnClickListener {
            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://virtueinitiative.org")))
        }

        KeepAliveWorker.schedule(this)
        requestBackgroundFriendlySettings()
        refreshUi()
    }

    override fun onResume() {
        super.onResume()
        refreshUi()
        // Re-check after short delay in case the accessibility service just connected
        binding.root.postDelayed({ refreshUi() }, 600)
    }

    private fun login() {
        if (!saveOverrides(applyNow = true, showSavedMessage = false)) return

        val email = binding.emailInput.text?.toString()?.trim().orEmpty()
        val password = binding.passwordInput.text?.toString().orEmpty()
        val deviceName = binding.deviceNameInput.text?.toString()?.trim()
            ?.ifBlank { null } ?: deviceName()

        if (email.isBlank() || password.isBlank()) {
            setStatus("Email and password are required")
            return
        }

        binding.loginButton.isEnabled = false
        lifecycleScope.launch {
            val error = withContext(Dispatchers.IO) {
                var result = NativeBridge.nativeLogin(email, password, deviceName)
                if (result != null && result.contains("serialization error")) {
                    // Corrupted state files — wipe core-data and retry
                    android.util.Log.w("MainActivity", "Login serialization error, wiping core-data and retrying")
                    filesDir.resolve("core-data").deleteRecursively()
                    result = NativeBridge.nativeLogin(email, password, deviceName)
                }
                result
            }
            binding.loginButton.isEnabled = true

            if (error == null) {
                refreshUi()
                if (!VirtueAccessibilityService.isConnected()) {
                    showAccessibilityOnboarding()
                } else if (!VirtueAccessibilityService.isEnabled()) {
                    VirtueAccessibilityService.resume()
                    setStatus("Monitoring started.")
                    refreshUi()
                }
            } else {
                setStatus("Login failed: $error")
            }
        }
    }

    private fun logout() {
        androidx.appcompat.app.AlertDialog.Builder(this)
            .setTitle(getString(R.string.dialog_sign_out_title))
            .setMessage(getString(R.string.dialog_sign_out_message))
            .setPositiveButton(getString(R.string.btn_sign_out)) { _, _ ->
                lifecycleScope.launch {
                    withContext(Dispatchers.IO) {
                        NativeBridge.ensureInitialized(this@MainActivity)
                        val error = NativeBridge.nativeLogout()
                        if (error != null) {
                            // Native logout failed (e.g. corrupted auth file) — delete directly
                            android.util.Log.w("MainActivity", "nativeLogout failed ($error), deleting auth file directly")
                            runCatching {
                                filesDir.resolve("core-data/auth.json").delete()
                            }
                        }
                    }
                    VirtueAccessibilityService.pause()
                    ScreenshotService.stop(this@MainActivity)
                    refreshUi()
                }
            }
            .setNegativeButton(getString(R.string.dialog_cancel), null)
            .show()
    }

    private fun refreshUi() {
        val loggedIn = NativeBridge.nativeIsLoggedIn()
        binding.loginPanel.visibility = if (loggedIn) android.view.View.GONE else android.view.View.VISIBLE
        binding.sessionPanel.visibility = if (loggedIn) android.view.View.VISIBLE else android.view.View.GONE

        if (loggedIn) {
            binding.deviceIdText.text = deviceName()
            binding.statusButtonsLayout.visibility = android.view.View.VISIBLE

            when {
                VirtueAccessibilityService.isEnabled() -> {
                    binding.statusTitle.text = getString(R.string.status_monitoring)
                    binding.pauseResumeButton.text = getString(R.string.btn_pause_monitoring)
                    setStatus("Monitoring service is running")
                }
                VirtueAccessibilityService.isPaused() -> {
                    binding.statusTitle.text = getString(R.string.status_paused)
                    binding.pauseResumeButton.text = getString(R.string.btn_resume_monitoring)
                    setStatus("Monitoring is paused")
                }
                VirtueAccessibilityService.isConnected() -> {
                    binding.statusTitle.text = getString(R.string.status_waiting)
                    binding.pauseResumeButton.text = getString(R.string.btn_resume_monitoring)
                    setStatus("Accessibility service connected — tap Resume to start")
                }
                else -> {
                    binding.statusTitle.text = getString(R.string.status_waiting)
                    binding.pauseResumeButton.text = getString(R.string.btn_resume_monitoring)
                    setStatus("Enable Virtue in Accessibility Settings to start monitoring")
                }
            }
        } else {
            binding.statusTitle.text = getString(R.string.status_signed_out)
            binding.statusButtonsLayout.visibility = android.view.View.GONE
            setStatus(getString(R.string.msg_sign_in_to_start))
        }
    }

    private fun toggleMonitoring() {
        val isRunning = VirtueAccessibilityService.isEnabled()
        val title = if (isRunning) getString(R.string.dialog_pause_title) else getString(R.string.dialog_resume_title)
        val message = if (isRunning) getString(R.string.dialog_pause_message) else getString(R.string.dialog_resume_message)

        androidx.appcompat.app.AlertDialog.Builder(this)
            .setTitle(title)
            .setMessage(message)
            .setPositiveButton(getString(R.string.dialog_confirm)) { _, _ ->
                if (isRunning) {
                    lifecycleScope.launch(Dispatchers.IO) {
                        NativeBridge.nativeNoteUserStop("android_pause_button")
                        VirtueAccessibilityService.pause()
                        withContext(Dispatchers.Main) {
                            setStatus("Monitoring paused")
                            refreshUi()
                        }
                    }
                } else {
                    if (!VirtueAccessibilityService.isConnected()) {
                        showAccessibilityOnboarding()
                    } else {
                        VirtueAccessibilityService.resume()
                        setStatus("Monitoring resumed")
                        binding.root.postDelayed({ refreshUi() }, 400)
                    }
                }
            }
            .setNegativeButton(getString(R.string.dialog_cancel), null)
            .show()
    }

    private fun openAccessibilitySettings() {
        startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
    }

    private fun showAccessibilityOnboarding() {
        androidx.appcompat.app.AlertDialog.Builder(this)
            .setTitle("Enable Screen Monitoring")
            .setMessage(
                "Virtue needs Accessibility permission to monitor your screen.\n\n" +
                "1. Tap \"Open Settings\" below\n" +
                "2. Find \"Virtue\" in the list\n" +
                "3. Toggle it on and confirm\n\n" +
                "Monitoring will start automatically once enabled."
            )
            .setPositiveButton("Open Settings") { _, _ -> openAccessibilitySettings() }
            .setNegativeButton(getString(R.string.dialog_cancel), null)
            .show()
    }

    private fun requestBackgroundFriendlySettings() {
        if (Build.VERSION.SDK_INT >= 33) {
            val granted = checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) ==
                android.content.pm.PackageManager.PERMISSION_GRANTED
            if (!granted) {
                notificationPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
            }
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            val pm = getSystemService(PowerManager::class.java)
            if (!pm.isIgnoringBatteryOptimizations(packageName)) {
                runCatching {
                    startActivity(
                        Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS)
                            .setData(Uri.parse("package:$packageName"))
                    )
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

        if (showSavedMessage) setStatus("Overrides saved")
        return true
    }

    private fun showOverridesDialog() {
        val bgColor = 0xFFF9F9F7.toInt()
        val cardColor = 0xFFFFFFFF.toInt()
        val borderColor = 0xFFE5E5EA.toInt()
        val labelColor = 0xFF888888.toInt()
        val valueColor = 0xFF1A1C19.toInt()
        val dp = resources.displayMetrics.density

        fun cardDrawable() = GradientDrawable().apply {
            setColor(cardColor)
            cornerRadius = 18 * dp
            setStroke((1 * dp).toInt(), borderColor)
        }

        fun divider() = android.view.View(this).apply {
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, (1 * dp).toInt()
            ).apply { topMargin = (14 * dp).toInt(); bottomMargin = (14 * dp).toInt() }
            setBackgroundColor(borderColor)
        }

        fun makeField(label: String, value: String, numeric: Boolean): android.widget.EditText {
            return android.widget.EditText(this).apply {
                setText(value)
                hint = label
                setHintTextColor(labelColor)
                setTextColor(valueColor)
                textSize = 15f
                inputType = if (numeric)
                    android.text.InputType.TYPE_CLASS_NUMBER
                else
                    android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_URI
                background = null
                setPadding(0, (4 * dp).toInt(), 0, (4 * dp).toInt())
                layoutParams = LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT,
                    LinearLayout.LayoutParams.WRAP_CONTENT
                )
            }
        }

        val apiEdit = makeField(getString(R.string.override_api_url), binding.baseApiUrlInput.text.toString(), numeric = false)
        val captureEdit = makeField(getString(R.string.override_capture_interval), binding.captureIntervalInput.text.toString(), numeric = true)
        val batchEdit = makeField(getString(R.string.override_batch_window), binding.batchWindowInput.text.toString(), numeric = true)

        val card = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            background = cardDrawable()
            val pad = (20 * dp).toInt()
            setPadding(pad, pad, pad, pad)
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT
            )
        }
        card.addView(apiEdit)
        card.addView(divider())
        card.addView(captureEdit)
        card.addView(divider())
        card.addView(batchEdit)

        val cancelBtn = com.google.android.material.button.MaterialButton(
            this, null, com.google.android.material.R.attr.materialButtonOutlinedStyle
        ).apply {
            text = getString(R.string.dialog_cancel)
            setTextColor(0xFF008900.toInt())
            strokeColor = android.content.res.ColorStateList.valueOf(0xFF008900.toInt())
            backgroundTintList = android.content.res.ColorStateList.valueOf(android.graphics.Color.TRANSPARENT)
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT
            ).apply { bottomMargin = (8 * dp).toInt() }
        }
        val applyBtn = com.google.android.material.button.MaterialButton(this).apply {
            text = getString(R.string.btn_apply_overrides)
            backgroundTintList = android.content.res.ColorStateList.valueOf(0xFF008900.toInt())
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT
            )
        }

        val outerScroll = ScrollView(this).apply { setBackgroundColor(bgColor) }
        val outerContainer = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            val pad = (20 * dp).toInt()
            setPadding(pad, pad, pad, pad)
        }
        outerScroll.addView(outerContainer)

        outerContainer.addView(TextView(this).apply {
            text = getString(R.string.section_overrides)
            textSize = 20f
            setTextColor(valueColor)
            typeface = android.graphics.Typeface.DEFAULT_BOLD
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT
            ).apply { bottomMargin = (18 * dp).toInt() }
        })
        outerContainer.addView(card)
        outerContainer.addView(LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT
            ).apply { topMargin = (16 * dp).toInt() }
            addView(cancelBtn)
            addView(applyBtn)
        })

        val dialog = Dialog(this, android.R.style.Theme_Material_Light_NoActionBar_Fullscreen)
        dialog.requestWindowFeature(Window.FEATURE_NO_TITLE)
        dialog.setContentView(outerScroll)
        dialog.window?.apply {
            setLayout(
                android.view.WindowManager.LayoutParams.MATCH_PARENT,
                android.view.WindowManager.LayoutParams.MATCH_PARENT
            )
            setSoftInputMode(android.view.WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE)
        }

        cancelBtn.setOnClickListener { dialog.dismiss() }
        applyBtn.setOnClickListener {
            binding.baseApiUrlInput.setText(apiEdit.text)
            binding.captureIntervalInput.setText(captureEdit.text)
            binding.batchWindowInput.setText(batchEdit.text)
            if (saveOverrides(applyNow = true, showSavedMessage = true)) dialog.dismiss()
        }

        dialog.show()
    }

    private fun showStatusDetails() {
        val json = runCatching { JSONObject(NativeBridge.nativeGetStatusJson()) }.getOrElse { JSONObject() }
        val lifecycle = json.optJSONObject("lifecycle") ?: JSONObject()
        val snapshot = lifecycle.optJSONObject("snapshot") ?: JSONObject()

        val pendingRequests = json.optInt("pending_request_count", 0)
        val lastLoop = formatTimestampMs(json.optLong("last_loop_at_ms", 0))
        val lastScreenshot = formatTimestampMs(json.optLong("last_screenshot_at_ms", 0))
        val lastBatch = formatTimestampMs(json.optLong("last_batch_at_ms", 0))
        val userSession = snapshot.optString("user_session", "unknown")
        val primaryService = snapshot.optString("primary_service", "unknown")
        val capturePermission = snapshot.optString("capture_permission", "unknown")
        val captureAvailability = snapshot.optString("capture_availability", "unknown")

        val overrides = OverrideSettings.load(this)
        val apiUrl = overrides.baseApiUrl ?: "https://api.virtueinitiative.org"

        val bgColor = 0xFFF9F9F7.toInt()
        val cardColor = 0xFFFFFFFF.toInt()
        val borderColor = 0xFFE5E5EA.toInt()
        val labelColor = 0xFF888888.toInt()
        val valueColor = 0xFF1A1C19.toInt()
        val dp = resources.displayMetrics.density

        fun cardDrawable() = GradientDrawable().apply {
            setColor(cardColor)
            cornerRadius = 18 * dp
            setStroke((1 * dp).toInt(), borderColor)
        }

        fun makeCard(vararg rows: Pair<String, String>): LinearLayout {
            val card = LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                background = cardDrawable()
                val pad = (20 * dp).toInt()
                setPadding(pad, pad, pad, pad)
                layoutParams = LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT
                ).apply { bottomMargin = (18 * dp).toInt() }
            }
            rows.forEachIndexed { i, (label, value) ->
                if (i > 0) {
                    card.addView(android.view.View(this).apply {
                        layoutParams = LinearLayout.LayoutParams(
                            LinearLayout.LayoutParams.MATCH_PARENT, (1 * dp).toInt()
                        ).apply { topMargin = (12 * dp).toInt(); bottomMargin = (12 * dp).toInt() }
                        setBackgroundColor(borderColor)
                    })
                }
                card.addView(TextView(this).apply {
                    text = label; textSize = 11f; setTextColor(labelColor)
                })
                card.addView(TextView(this).apply {
                    text = value.ifEmpty { "—" }; textSize = 14f; setTextColor(valueColor)
                    layoutParams = LinearLayout.LayoutParams(
                        LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT
                    ).apply { topMargin = (2 * dp).toInt() }
                })
            }
            return card
        }

        fun sectionLabel(title: String) = TextView(this).apply {
            text = title.uppercase(); textSize = 11f; setTextColor(labelColor)
            typeface = android.graphics.Typeface.DEFAULT_BOLD
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT
            ).apply { bottomMargin = (8 * dp).toInt() }
        }

        val outerScroll = ScrollView(this).apply { setBackgroundColor(bgColor) }
        val outerContainer = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            val pad = (20 * dp).toInt()
            setPadding(pad, pad, pad, pad)
        }
        outerScroll.addView(outerContainer)

        val headingRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = android.view.Gravity.CENTER_VERTICAL
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT
            ).apply { bottomMargin = (18 * dp).toInt() }
        }
        headingRow.addView(TextView(this).apply {
            text = getString(R.string.btn_status_details); textSize = 20f; setTextColor(valueColor)
            typeface = android.graphics.Typeface.DEFAULT_BOLD
            layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
        })
        val doneBtn = com.google.android.material.button.MaterialButton(this).apply {
            text = getString(R.string.btn_done)
            setTextColor(0xFF008900.toInt())
            strokeColor = android.content.res.ColorStateList.valueOf(0xFF008900.toInt())
            strokeWidth = (1 * dp).toInt()
            backgroundTintList = android.content.res.ColorStateList.valueOf(android.graphics.Color.TRANSPARENT)
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT
            )
        }
        headingRow.addView(doneBtn)
        outerContainer.addView(headingRow)

        val a11yStatus = when {
            VirtueAccessibilityService.isEnabled() -> "Active"
            VirtueAccessibilityService.isPaused() -> "Paused"
            VirtueAccessibilityService.isConnected() -> "Connected (not running)"
            else -> "Not enabled"
        }

        outerContainer.addView(sectionLabel("Service"))
        outerContainer.addView(makeCard(
            "Summary" to binding.statusTitle.text.toString(),
            "Status" to binding.statusText.text.toString(),
            "Accessibility service" to a11yStatus,
            "Pending requests" to pendingRequests.toString(),
            "API" to apiUrl,
        ))

        outerContainer.addView(sectionLabel("Core Lifecycle"))
        outerContainer.addView(makeCard(
            "User session" to userSession,
            "Primary service" to primaryService,
            "Capture permission" to capturePermission,
            "Capture availability" to captureAvailability,
        ))

        outerContainer.addView(sectionLabel("Timing"))
        outerContainer.addView(makeCard(
            "Last loop" to lastLoop,
            "Last screenshot" to lastScreenshot,
            "Last batch" to lastBatch,
        ))

        val dialog = Dialog(this, android.R.style.Theme_Material_Light_NoActionBar_Fullscreen)
        dialog.requestWindowFeature(Window.FEATURE_NO_TITLE)
        dialog.setContentView(outerScroll)
        dialog.window?.setLayout(
            android.view.WindowManager.LayoutParams.MATCH_PARENT,
            android.view.WindowManager.LayoutParams.MATCH_PARENT
        )
        doneBtn.setOnClickListener { dialog.dismiss() }
        dialog.show()
    }

    private fun formatTimestampMs(ms: Long): String {
        if (ms <= 0L) return "—"
        return SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.getDefault()).format(Date(ms))
    }
}
