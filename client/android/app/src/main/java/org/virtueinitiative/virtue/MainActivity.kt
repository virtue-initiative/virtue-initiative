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

        if (binding.deviceNameInput.text.isNullOrBlank()) {
            binding.deviceNameInput.setText(deviceName())
        }

        val initError = NativeBridge.ensureInitialized(this)
        if (initError != null) {
            setStatus("Core init failed: $initError")
        }

        binding.loginButton.setOnClickListener { login() }
        binding.signOutButton.setOnClickListener { logout() }
        binding.statusDetailsButton.setOnClickListener { showStatusDetails() }
        binding.pauseResumeButton.setOnClickListener { toggleMonitoring() }
        binding.forceCaptureButton.setOnClickListener { forceCapture() }
        binding.openAccessibilitySettingsButton.setOnClickListener { openAccessibilitySettings() }

        binding.websiteLink.setOnClickListener {
            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://virtueinitiative.org")))
        }

        binding.reportBugLink.setOnClickListener { showReportBugDialog() }

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
        // nativeLogin() blocks waiting for a reply from the daemon loop thread,
        // so the loop must already be running before we call it. The loop is
        // only ever started by the accessibility service (on connect, or via
        // resume() after a prior logout paused it) — require that first rather
        // than attempting a login that can only time out.
        if (!VirtueAccessibilityService.isConnected()) {
            showAccessibilityOnboarding()
            return
        }
        if (!VirtueAccessibilityService.isEnabled()) {
            VirtueAccessibilityService.resume()
        }

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
                AccountEmailStore.save(this@MainActivity, email)
                refreshUi()
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
                    AccountEmailStore.clear(this@MainActivity)
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
        val accessibilityConnected = VirtueAccessibilityService.isConnected()

        // First-install flow: nothing else can happen (login blocks waiting on the
        // daemon loop, which the accessibility service is what starts) until the
        // service is connected, so keep signed-out users on the onboarding screen
        // until then.
        if (!loggedIn && !accessibilityConnected) {
            binding.onboardingPanel.visibility = android.view.View.VISIBLE
            binding.loginPanel.visibility = android.view.View.GONE
            binding.sessionPanel.visibility = android.view.View.GONE
            binding.statusButtonsLayout.visibility = android.view.View.GONE
            binding.onboardingStatusText.text = getString(R.string.msg_onboarding_waiting)
            binding.statusTitle.text = getString(R.string.status_signed_out)
            setStatus(getString(R.string.msg_sign_in_to_start))
            return
        }

        if (!loggedIn) {
            // Accessibility just connected (or already was) — make sure the core
            // actually finished initializing before handing off to the login screen,
            // since login() blocks on a daemon loop thread that only the core's
            // successful init can start.
            val initError = NativeBridge.ensureInitialized(this)
            val coreReady = initError == null
            binding.onboardingPanel.visibility = if (coreReady) android.view.View.GONE else android.view.View.VISIBLE
            binding.loginPanel.visibility = if (coreReady) android.view.View.VISIBLE else android.view.View.GONE
            binding.sessionPanel.visibility = android.view.View.GONE
            binding.statusButtonsLayout.visibility = android.view.View.GONE
            if (!coreReady) {
                binding.onboardingStatusText.text = getString(R.string.msg_core_init_failed, initError)
            }
            binding.statusTitle.text = getString(R.string.status_signed_out)
            setStatus(getString(R.string.msg_sign_in_to_start))
            return
        }

        binding.onboardingPanel.visibility = android.view.View.GONE
        binding.loginPanel.visibility = android.view.View.GONE
        binding.sessionPanel.visibility = android.view.View.VISIBLE

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

    private fun forceCapture() {
        binding.forceCaptureButton.isEnabled = false
        setStatus(getString(R.string.msg_force_capture_started))
        lifecycleScope.launch {
            val error = withContext(Dispatchers.IO) {
                NativeBridge.nativeForceCapture()
            }
            binding.forceCaptureButton.isEnabled = true

            if (error == null) {
                setStatus(getString(R.string.msg_force_capture_succeeded))
            } else {
                setStatus(getString(R.string.msg_force_capture_failed, error))
            }
            // Transient confirmation/error — revert to the real status after a beat
            // rather than leaving it stuck here, matching toggleMonitoring's pattern.
            binding.root.postDelayed({ refreshUi() }, 2500)
        }
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
                "3. Toggle it on and confirm\n" +
                "4. Return here to sign in\n\n" +
                "Monitoring will start automatically once you're signed in."
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

    private fun showStatusDetails() {
        // Every field here comes from the shared core status payload
        // (client/core/SPEC.md CORE-010), so this screen shows the same things
        // as the desktop clients', plus the Android-specific rows below.
        val json = runCatching { JSONObject(NativeBridge.nativeGetStatusJson()) }.getOrElse { JSONObject() }

        // `optString` renders a JSON null as the literal "null", so absent and
        // null values are both routed through `isNull` first.
        fun optText(key: String): String =
            if (json.isNull(key)) "—" else json.optString(key, "").ifBlank { "—" }
        fun optTime(key: String): String = formatTimestampMs(json.optLong(key, 0))

        val accountEmail = optText("account_email")
        val deviceName = optText("device_name")
        val partners = if (json.isNull("partner_count")) "unknown" else json.optInt("partner_count", 0).toString()
        val pendingHash = json.optInt("pending_hash_count", 0)
        val pendingBatch = json.optInt("pending_batch_count", 0)
        val pendingRequests = json.optInt("pending_request_count", 0)
        val lastLoop = optTime("last_loop_at_ms")
        val lastAttempt = optTime("last_screenshot_attempt_at_ms")
        val lastScreenshot = optTime("last_screenshot_at_ms")
        val lastBatch = optTime("last_batch_at_ms")
        val skipReason = when (if (json.isNull("last_skip_reason")) "" else json.optString("last_skip_reason", "")) {
            "static_screen" -> "Screen unchanged since the last upload"
            "locked_or_screensaver" -> "Screen locked or screensaver active"
            "capture_failed" -> "Capture failed"
            else -> "—"
        }
        val recentErrors = json.optJSONArray("recent_errors")
        val apiBaseUrl = optText("api_base_url")
        val hashBaseUrl = if (json.isNull("hash_base_url")) "default" else json.optString("hash_base_url", "").ifBlank { "default" }
        val captureInterval = "${json.optLong("capture_interval_seconds", 0)}s"
        val batchWindow = "${json.optLong("batch_window_seconds", 0)}s"
        val deviceId = optText("device_id")
        val logFile = java.io.File(filesDir, "core-data/logs")
            .listFiles()
            ?.maxByOrNull { it.lastModified() }

        val bgColor = 0xFFF4EFE3.toInt()
        val cardColor = 0xFFFBF7EA.toInt()
        val borderColor = 0xFFD9D1BC.toInt()
        val labelColor = 0xFF9C9682.toInt()
        val valueColor = 0xFF1B1A16.toInt()
        val dp = resources.displayMetrics.density

        fun cardDrawable() = GradientDrawable().apply {
            setColor(cardColor)
            cornerRadius = 4 * dp
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
            setTextColor(0xFF1E3A2E.toInt())
            strokeColor = android.content.res.ColorStateList.valueOf(0xFF1E3A2E.toInt())
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

        outerContainer.addView(sectionLabel("Account"))
        outerContainer.addView(makeCard(
            "Summary" to binding.statusTitle.text.toString(),
            "Status" to binding.statusText.text.toString(),
            "Email" to accountEmail,
            "Device name" to deviceName,
            "Partners" to partners,
        ))

        outerContainer.addView(sectionLabel("Queues"))
        outerContainer.addView(makeCard(
            "Waiting for hash" to pendingHash.toString(),
            "Waiting in batch" to pendingBatch.toString(),
            "Pending requests" to pendingRequests.toString(),
            "Last batch upload" to lastBatch,
        ))

        outerContainer.addView(sectionLabel("Capture"))
        outerContainer.addView(makeCard(
            "Accessibility service" to a11yStatus,
            "Last loop" to lastLoop,
            "Last attempt" to lastAttempt,
            "Last screenshot" to lastScreenshot,
            "Last skip reason" to skipReason,
        ))

        outerContainer.addView(sectionLabel("Recent errors"))
        val errorRows = mutableListOf<Pair<String, String>>()
        if (recentErrors != null) {
            for (i in 0 until minOf(recentErrors.length(), 5)) {
                val error = recentErrors.optJSONObject(i) ?: continue
                errorRows.add(
                    "${formatTimestampMs(error.optLong("at_ms", 0))} · ${error.optString("context")}"
                        to error.optString("message")
                )
            }
        }
        outerContainer.addView(
            if (errorRows.isEmpty()) makeCard("Errors" to "None")
            else makeCard(*errorRows.toTypedArray())
        )

        outerContainer.addView(sectionLabel("Advanced"))
        outerContainer.addView(makeCard(
            "Device ID" to deviceId,
            "API URL" to apiBaseUrl,
            "Hash base URL" to hashBaseUrl,
            "Capture interval" to captureInterval,
            "Batch window" to batchWindow,
            "Log file" to (logFile?.absolutePath ?: "—"),
        ))

        val openLogBtn = com.google.android.material.button.MaterialButton(this).apply {
            text = getString(R.string.btn_open_log)
            setTextColor(0xFF1E3A2E.toInt())
            strokeColor = android.content.res.ColorStateList.valueOf(0xFF1E3A2E.toInt())
            strokeWidth = (1 * dp).toInt()
            backgroundTintList = android.content.res.ColorStateList.valueOf(android.graphics.Color.TRANSPARENT)
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT
            )
            setOnClickListener { openLogFile(logFile) }
        }
        outerContainer.addView(openLogBtn)

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

    private fun showReportBugDialog() {
        val dp = resources.displayMetrics.density

        val descriptionLayout = com.google.android.material.textfield.TextInputLayout(this).apply {
            boxBackgroundMode = com.google.android.material.textfield.TextInputLayout.BOX_BACKGROUND_OUTLINE
        }
        val descriptionInput = com.google.android.material.textfield.TextInputEditText(descriptionLayout.context).apply {
            hint = getString(R.string.report_bug_description_hint)
            isSingleLine = false
            minLines = 3
            maxLines = 6
        }
        descriptionLayout.addView(descriptionInput)

        val emailLayout = com.google.android.material.textfield.TextInputLayout(this).apply {
            boxBackgroundMode = com.google.android.material.textfield.TextInputLayout.BOX_BACKGROUND_OUTLINE
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT
            ).apply { topMargin = (12 * dp).toInt() }
        }
        val emailInput = com.google.android.material.textfield.TextInputEditText(emailLayout.context).apply {
            hint = getString(R.string.report_bug_contact_email_hint)
            inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS
            setText(AccountEmailStore.load(this@MainActivity).orEmpty())
        }
        emailLayout.addView(emailInput)

        val includeLogsCheckBox = android.widget.CheckBox(this).apply {
            text = getString(R.string.report_bug_include_logs)
            isChecked = true
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT
            ).apply { topMargin = (14 * dp).toInt() }
        }

        val includeLogsCaption = TextView(this).apply {
            text = getString(R.string.report_bug_include_logs_caption)
            textSize = 12f
            setTextColor(0xFF9C9682.toInt())
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT
            ).apply { topMargin = (4 * dp).toInt() }
        }

        val errorText = TextView(this).apply {
            setTextColor(0xFFEF4444.toInt())
            visibility = android.view.View.GONE
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT
            ).apply { topMargin = (10 * dp).toInt() }
        }

        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            val pad = (20 * dp).toInt()
            setPadding(pad, pad, pad, 0)
            addView(descriptionLayout)
            addView(emailLayout)
            addView(includeLogsCheckBox)
            addView(includeLogsCaption)
            addView(errorText)
        }

        val dialog = androidx.appcompat.app.AlertDialog.Builder(this)
            .setTitle(getString(R.string.dialog_report_bug_title))
            .setView(content)
            .setPositiveButton(getString(R.string.btn_send_report), null)
            .setNegativeButton(getString(R.string.dialog_cancel), null)
            .create()

        dialog.setOnShowListener {
            val sendButton = dialog.getButton(android.app.AlertDialog.BUTTON_POSITIVE)
            sendButton.setOnClickListener {
                val message = descriptionInput.text?.toString()?.trim().orEmpty()
                if (message.isEmpty()) {
                    errorText.text = getString(R.string.report_bug_message_required)
                    errorText.visibility = android.view.View.VISIBLE
                    return@setOnClickListener
                }

                val contactEmail = emailInput.text?.toString()?.trim().orEmpty()
                val includeLogs = includeLogsCheckBox.isChecked
                val platformDetails = androidPlatformDetails()

                sendButton.isEnabled = false
                lifecycleScope.launch {
                    val error = withContext(Dispatchers.IO) {
                        NativeBridge.nativeReportIssue(message, contactEmail, includeLogs, platformDetails)
                    }
                    sendButton.isEnabled = true

                    if (error == null) {
                        dialog.dismiss()
                        showReportSentDialog()
                    } else {
                        errorText.text = getString(R.string.report_bug_send_failed)
                        errorText.visibility = android.view.View.VISIBLE
                    }
                }
            }
        }

        dialog.show()
    }

    private fun showReportSentDialog() {
        androidx.appcompat.app.AlertDialog.Builder(this)
            .setTitle(getString(R.string.dialog_report_sent_title))
            .setMessage(getString(R.string.dialog_report_sent_message))
            .setPositiveButton(getString(R.string.btn_done), null)
            .show()
    }

    private fun androidPlatformDetails(): String {
        return "Android ${Build.VERSION.RELEASE} (SDK ${Build.VERSION.SDK_INT}); ${deviceName()}"
    }

    /**
     * Opens the newest rolling log file in a text viewer — an app-internal file
     * is otherwise unreachable to the user. Falls back to the share sheet when
     * no installed app can view `text/plain`, since that at least gets the file
     * somewhere the user can read it.
     */
    private fun openLogFile(logFile: java.io.File?) {
        if (logFile == null || !logFile.exists()) {
            setStatus(getString(R.string.open_log_unavailable))
            return
        }
        // The rolling log files are named `virtue.<date>` with no extension, so
        // a viewer that sniffs the type from the file name (rather than the
        // intent's) refuses to render them. Hand out a `.txt` copy instead —
        // which also keeps the viewer off a file the daemon is still appending to.
        val readable = java.io.File(cacheDir, "logs").let { dir ->
            dir.mkdirs()
            java.io.File(dir, "virtue-log.txt").also { logFile.copyTo(it, overwrite = true) }
        }
        val uri = androidx.core.content.FileProvider.getUriForFile(
            this,
            "$packageName.fileprovider",
            readable,
        )
        val view = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "text/plain")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        if (view.resolveActivity(packageManager) != null) {
            startActivity(Intent.createChooser(view, getString(R.string.open_log_chooser)))
            return
        }

        // No viewer installed (a bare emulator image, say) — hand the file to
        // whatever can take it instead, so the log still gets somewhere readable.
        val share = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_STREAM, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        try {
            startActivity(Intent.createChooser(share, getString(R.string.open_log_chooser)))
        } catch (e: android.content.ActivityNotFoundException) {
            android.util.Log.w("MainActivity", "no app can open or receive the log file", e)
            setStatus(getString(R.string.open_log_no_viewer))
        }
    }

    private fun formatTimestampMs(ms: Long): String {
        if (ms <= 0L) return "—"
        return SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.getDefault()).format(Date(ms))
    }
}
