package org.virtueinitiative.virtue

import android.Manifest
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import android.text.InputType
import android.text.SpannableString
import android.text.Spannable
import android.text.format.DateUtils
import android.text.style.ForegroundColorSpan
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import androidx.activity.OnBackPressedCallback
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.app.NotificationManagerCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.updateLayoutParams
import androidx.core.widget.doAfterTextChanged
import androidx.lifecycle.lifecycleScope
import com.google.android.material.checkbox.MaterialCheckBox
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import com.google.android.material.textfield.TextInputEditText
import com.google.android.material.textfield.TextInputLayout
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import org.virtueinitiative.virtue.databinding.ActivityMainBinding
import java.text.DateFormat
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

class MainActivity : AppCompatActivity() {
    private lateinit var binding: ActivityMainBinding

    private enum class Screen { HOME, SETUP_STEPS, STATUS_DETAILS, MONITORS, REPORT }

    /** The one thing Home shows, chosen by the headline state (CLIENT-030). */
    private enum class State { A11Y_OFF, SIGNED_OUT, PAUSED, ACTIVE, WAITING, NOT_RUNNING }

    private var screen = Screen.HOME
    private var lastHomeKey: String? = null
    private var advancedOpen = false

    // Sign-in form state. The form is rebuilt when Home refreshes, so what the
    // user typed lives here rather than in the views.
    private var emailDraft = ""
    private var passwordDraft = ""
    private var deviceDraft = ""
    private var loginError: String? = null
    private var signingIn = false
    private var loginErrorView: TextView? = null
    private var loginButton: com.google.android.material.button.MaterialButton? = null

    // Transient line in Status Details > Advanced (test screenshot, log file).
    private var detailsMessage: String? = null
    private var detailsMessageView: TextView? = null
    private var testButton: com.google.android.material.button.MaterialButton? = null

    private lateinit var brandView: View
    private val backCallback = object : OnBackPressedCallback(false) {
        override fun handleOnBackPressed() = showScreen(Screen.HOME)
    }

    private val notificationPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)
        applyWindowInsets()
        onBackPressedDispatcher.addCallback(this, backCallback)
        brandView = buildBrandView()
        binding.toolbar.addView(brandView)
        binding.toolbar.setOnMenuItemClickListener(::onMenuItem)

        val initError = NativeBridge.ensureInitialized(this)
        if (initError != null) {
            loginError = getString(R.string.msg_core_init_failed, initError)
        }

        KeepAliveWorker.schedule(this)
        AppUpdater.checkSoon(this)
        requestBackgroundFriendlySettings()
        showScreen(Screen.HOME)
        handleIntent(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleIntent(intent)
    }

    private fun handleIntent(intent: Intent?) {
        if (intent?.action == AppUpdater.ACTION_INSTALL_UPDATE) {
            // Consume it so a rotation or recreation doesn't start a second install.
            intent.action = null
            installUpdate()
        }
    }

    private fun installUpdate() {
        lifecycleScope.launch {
            val started = withContext(Dispatchers.IO) { AppUpdater.installReadyUpdate(this@MainActivity) }
            if (!started) refreshUi()
        }
    }

    // Home follows the monitoring state while it is on screen, so "Waiting for
    // first capture" turns into "Monitoring active" without leaving the app.
    private val homePoll = object : Runnable {
        override fun run() {
            if (screen == Screen.HOME) refreshUi()
            binding.root.postDelayed(this, 3000)
        }
    }

    override fun onStart() {
        super.onStart()
        AppUpdater.uiVisible = true
        binding.root.postDelayed(homePoll, 3000)
    }

    override fun onStop() {
        super.onStop()
        AppUpdater.uiVisible = false
        binding.root.removeCallbacks(homePoll)
    }

    override fun onResume() {
        super.onResume()
        refreshUi()
        // Re-check after short delay in case the accessibility service just connected
        binding.root.postDelayed({ refreshUi() }, 600)
    }

    // targetSdk 35 forces edge-to-edge, so the window no longer shrinks for the
    // keyboard (adjustResize) or stops below the status bar on its own. Inset the
    // root by hand. Using margins rather than padding resizes the scroll view, and
    // a scroll view that shrinks scrolls its focused field back into view.
    private fun applyWindowInsets() {
        ViewCompat.setOnApplyWindowInsetsListener(binding.root) { view, insets ->
            val bars = insets.getInsets(
                WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
            )
            val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
            view.updateLayoutParams<ViewGroup.MarginLayoutParams> {
                topMargin = bars.top
                leftMargin = bars.left
                rightMargin = bars.right
                bottomMargin = maxOf(bars.bottom, ime.bottom)
            }
            WindowInsetsCompat.CONSUMED
        }
    }

    // ---- Navigation -------------------------------------------------------

    private fun showScreen(next: Screen) {
        screen = next
        backCallback.isEnabled = next != Screen.HOME
        currentFocus?.clearFocus()
        configureToolbar()
        render()
        binding.scroll.scrollTo(0, 0)
    }

    private fun configureToolbar() {
        val toolbar = binding.toolbar
        toolbar.menu.clear()
        if (screen == Screen.HOME) {
            toolbar.title = ""
            toolbar.navigationIcon = null
            toolbar.setNavigationOnClickListener(null)
            brandView.visibility = View.VISIBLE
            toolbar.inflateMenu(R.menu.home_menu)
            toolbar.menu.findItem(R.id.menu_sign_out).title = tinted(getString(R.string.btn_sign_out), R.color.bad)
            toolbar.menu.findItem(R.id.menu_build).title =
                tinted(getString(R.string.menu_build, BuildConfig.VIRTUE_BUILD_LABEL), R.color.ink_3)
            toolbar.menu.findItem(R.id.menu_sign_out).isVisible = NativeBridge.nativeIsLoggedIn()
        } else {
            brandView.visibility = View.GONE
            toolbar.title = when (screen) {
                Screen.SETUP_STEPS -> getString(R.string.menu_setup_steps)
                Screen.STATUS_DETAILS -> getString(R.string.btn_status_details)
                Screen.MONITORS -> getString(R.string.menu_what_virtue_monitors)
                else -> getString(R.string.btn_report_bug)
            }
            toolbar.setNavigationIcon(R.drawable.ic_arrow_back)
            toolbar.setNavigationContentDescription(R.string.nav_back)
            toolbar.setNavigationOnClickListener { onBackPressedDispatcher.onBackPressed() }
        }
    }

    private fun tinted(text: String, color: Int) = SpannableString(text).apply {
        setSpan(ForegroundColorSpan(getColor(color)), 0, length, Spannable.SPAN_EXCLUSIVE_EXCLUSIVE)
    }

    private fun onMenuItem(item: android.view.MenuItem): Boolean {
        when (item.itemId) {
            R.id.menu_setup_steps -> showScreen(Screen.SETUP_STEPS)
            R.id.menu_status_details -> showScreen(Screen.STATUS_DETAILS)
            R.id.menu_what_virtue_monitors -> showScreen(Screen.MONITORS)
            R.id.menu_report_issue -> showScreen(Screen.REPORT)
            R.id.menu_sign_out -> logout()
            else -> return false
        }
        return true
    }

    /** The V logo and "Virtue Initiative", together one link to the website. */
    private fun buildBrandView(): View = LinearLayout(this).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        isClickable = true
        isFocusable = true
        minimumHeight = dp(48)
        val outValue = android.util.TypedValue()
        theme.resolveAttribute(android.R.attr.selectableItemBackground, outValue, true)
        setBackgroundResource(outValue.resourceId)
        addView(ImageView(context).apply {
            setImageResource(R.drawable.ic_logo)
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
            layoutParams = LinearLayout.LayoutParams(dp(28), dp(30)).apply { marginEnd = dp(10) }
        })
        addView(TextView(context).apply {
            text = getString(R.string.app_title)
            setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, 20f)
            typeface = android.graphics.Typeface.create("sans-serif-medium", android.graphics.Typeface.NORMAL)
            setTextColor(color(R.color.ink))
        })
        contentDescription = getString(R.string.app_title)
        setOnClickListener {
            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://virtueinitiative.org")))
        }
        layoutParams = androidx.appcompat.widget.Toolbar.LayoutParams(
            ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.START or Gravity.CENTER_VERTICAL
        ).apply { marginStart = dp(16) }
    }

    // ---- Rendering --------------------------------------------------------

    private fun refreshUi() {
        if (screen == Screen.REPORT || screen == Screen.MONITORS) return
        val y = binding.scroll.scrollY
        if (screen == Screen.HOME) {
            binding.toolbar.menu.findItem(R.id.menu_sign_out)?.isVisible = NativeBridge.nativeIsLoggedIn()
        }
        render()
        if (y > 0) binding.scroll.post { binding.scroll.scrollTo(0, y) }
    }

    private fun render() {
        if (screen != Screen.HOME) lastHomeKey = null
        detailsMessageView = null
        testButton = null
        when (screen) {
            Screen.HOME -> renderHome()
            Screen.SETUP_STEPS -> renderSetupSteps()
            Screen.STATUS_DETAILS -> renderStatusDetails()
            Screen.MONITORS -> renderMonitors()
            Screen.REPORT -> renderReport()
        }
    }

    private fun statusJson(): JSONObject =
        runCatching { JSONObject(NativeBridge.nativeGetStatusJson()) }.getOrElse { JSONObject() }

    private fun lastScreenshotMs(json: JSONObject): Long = json.optLong("last_screenshot_at_ms", 0)

    private fun currentState(json: JSONObject = statusJson()): State {
        val connected = VirtueAccessibilityService.isConnected()
        return when {
            !connected -> State.A11Y_OFF
            !NativeBridge.nativeIsLoggedIn() -> State.SIGNED_OUT
            VirtueAccessibilityService.isPaused() -> State.PAUSED
            VirtueAccessibilityService.isEnabled() ->
                if (lastScreenshotMs(json) > 0) State.ACTIVE else State.WAITING
            else -> State.NOT_RUNNING
        }
    }

    private fun headline(state: State): String = getString(
        when (state) {
            State.A11Y_OFF -> R.string.status_a11y_off
            State.SIGNED_OUT -> R.string.status_signed_out
            State.PAUSED -> R.string.status_paused
            State.ACTIVE -> R.string.status_monitoring
            State.WAITING, State.NOT_RUNNING -> R.string.status_waiting
        }
    )

    private fun tone(state: State): Tone = when (state) {
        State.A11Y_OFF -> Tone.BAD
        State.SIGNED_OUT -> Tone.NEUTRAL
        State.ACTIVE -> Tone.GOOD
        else -> Tone.WARN
    }

    private fun formatClock(ms: Long): String {
        val time = DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(ms))
        return if (DateUtils.isToday(ms)) time
        else "${DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(ms))}, $time"
    }

    private fun renderHome() {
        val content = binding.content
        val json = statusJson()
        val state = currentState(json)

        // The sign-in form keeps focus and the keyboard while the user types, so
        // a refresh that changes nothing must not rebuild it.
        val partners = if (json.isNull("partner_count")) 0 else json.optInt("partner_count", 0)
        val key = "${state.name}/${lastScreenshotMs(json)}/${partners > 0}/${AppUpdater.readyVersion(this)}"
        if (lastHomeKey == key && content.childCount > 0) return
        lastHomeKey = key

        content.removeAllViews()
        loginErrorView = null
        loginButton = null

        AppUpdater.readyVersion(this)?.let { version ->
            content.addView(tonalButton(getString(R.string.btn_install_update, version)) { installUpdate() }.topMargin(8))
        }

        when (state) {
            State.A11Y_OFF -> {
                val guide = LinearLayout(this).apply {
                    orientation = LinearLayout.VERTICAL
                    layoutParams = LinearLayout.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
                    ).apply { topMargin = dp(8) }
                }
                content.addView(guide)
                AccessibilitySetupGuide.renderGuide(this, guide, ::handleGuideAction)
            }
            State.SIGNED_OUT -> renderSignIn(content)
            else -> renderStatusFocus(content, state, json)
        }
    }

    private fun renderStatusFocus(content: LinearLayout, state: State, json: JSONObject) {
        val (icon, sentence) = when (state) {
            State.ACTIVE -> R.drawable.ic_ring_check to
                getString(R.string.sentence_active, formatClock(lastScreenshotMs(json)))
            State.WAITING -> R.drawable.ic_ring_dots to getString(R.string.sentence_waiting)
            State.NOT_RUNNING -> R.drawable.ic_ring_dots to getString(R.string.sentence_not_running)
            else -> R.drawable.ic_ring_pause to getString(R.string.sentence_paused)
        }
        content.addView(statusRing(tone(state), icon).topMargin(24))
        content.addView(text(headline(state), 26f, bold = true).apply {
            gravity = Gravity.CENTER
            ViewCompat.setAccessibilityHeading(this, true)
        }.topMargin(20))
        content.addView(text(sentence, 16f, R.color.ink_3).apply { gravity = Gravity.CENTER }.topMargin(8))

        val running = state == State.ACTIVE || state == State.WAITING
        if (running) {
            val partners = if (json.isNull("partner_count")) 0 else json.optInt("partner_count", 0)
            content.addView(
                filledButton(
                    getString(if (partners > 0) R.string.btn_add_more_partners else R.string.btn_add_partners)
                ) {
                    // Partners are managed in the web app, not from this client.
                    startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://app.virtueinitiative.org/partners")))
                }.topMargin(28)
            )
            content.addView(tonalButton(getString(R.string.btn_pause_monitoring)) { toggleMonitoring() }.topMargin(12))
        } else {
            content.addView(
                filledButton(getString(R.string.btn_resume_monitoring)) { toggleMonitoring() }.topMargin(28)
            )
        }
    }

    // ---- Sign in ----------------------------------------------------------

    private fun outlinedField(hint: String, input: Int, ime: Int, autofill: String?): Pair<TextInputLayout, TextInputEditText> {
        val layout = TextInputLayout(
            this, null, com.google.android.material.R.attr.textInputOutlinedStyle
        ).apply {
            this.hint = hint
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
            ).apply { topMargin = dp(12) }
        }
        val edit = TextInputEditText(layout.context).apply {
            inputType = input
            imeOptions = ime
            setSingleLine()
            if (autofill != null) setAutofillHints(autofill)
        }
        layout.addView(edit)
        return layout to edit
    }

    private fun renderSignIn(content: LinearLayout) {
        val initError = NativeBridge.ensureInitialized(this)
        if (initError != null) {
            content.addView(statusRing(Tone.BAD, R.drawable.ic_ring_alert).topMargin(24))
            content.addView(text(getString(R.string.status_core_failed), 26f, bold = true).apply {
                gravity = Gravity.CENTER
            }.topMargin(20))
            content.addView(
                text(getString(R.string.msg_core_init_failed, initError), 16f, R.color.ink_3)
                    .apply { gravity = Gravity.CENTER }.topMargin(8)
            )
            return
        }

        content.addView(text(getString(R.string.sign_in_title), 26f, bold = true).apply {
            ViewCompat.setAccessibilityHeading(this, true)
        }.topMargin(8))
        content.addView(text(getString(R.string.sign_in_sentence), 16f, R.color.ink_3).topMargin(8))

        val (emailLayout, email) = outlinedField(
            getString(R.string.email_hint),
            InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
            EditorInfo.IME_ACTION_NEXT,
            View.AUTOFILL_HINT_EMAIL_ADDRESS,
        )
        emailLayout.topMargin(20)
        val (passwordLayout, password) = outlinedField(
            getString(R.string.password_hint),
            InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD,
            EditorInfo.IME_ACTION_NEXT,
            View.AUTOFILL_HINT_PASSWORD,
        )
        passwordLayout.endIconMode = TextInputLayout.END_ICON_PASSWORD_TOGGLE
        val (deviceLayout, device) = outlinedField(
            getString(R.string.device_name_hint_default, deviceName()),
            InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_WORDS,
            EditorInfo.IME_ACTION_DONE,
            null,
        )
        email.setText(emailDraft)
        password.setText(passwordDraft)
        device.setText(deviceDraft)
        email.doAfterTextChanged { emailDraft = it?.toString().orEmpty(); showLoginError(null) }
        password.doAfterTextChanged { passwordDraft = it?.toString().orEmpty(); showLoginError(null) }
        device.doAfterTextChanged { deviceDraft = it?.toString().orEmpty() }
        device.setOnEditorActionListener { _, actionId, _ ->
            if (actionId == EditorInfo.IME_ACTION_DONE) { login(); true } else false
        }
        content.addView(emailLayout)
        content.addView(passwordLayout)
        content.addView(deviceLayout)

        val error = text("", 14f, R.color.bad).apply {
            accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
        }.topMargin(12) as TextView
        loginErrorView = error
        content.addView(error)
        showLoginError(loginError)

        val button = filledButton(getString(if (signingIn) R.string.btn_signing_in else R.string.btn_sign_in)) { login() }
        button.isEnabled = !signingIn
        loginButton = button
        content.addView(button.topMargin(16))
        content.addView(linkText(getString(R.string.btn_sign_up)) {
            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://app.virtueinitiative.org/signup")))
        }.topMargin(8))
    }

    private fun showLoginError(message: String?) {
        loginError = message
        loginErrorView?.text = message.orEmpty()
        loginErrorView?.visibility = if (message == null) View.GONE else View.VISIBLE
    }

    private fun login() {
        // nativeLogin() blocks waiting for a reply from the daemon loop thread,
        // so the loop must already be running before we call it. The loop is
        // only ever started by the accessibility service (on connect, or via
        // resume() after a prior logout paused it) — require that first rather
        // than attempting a login that can only time out.
        if (!VirtueAccessibilityService.isConnected()) {
            refreshUi()
            return
        }
        if (!VirtueAccessibilityService.isEnabled()) {
            VirtueAccessibilityService.resume()
        }

        val email = emailDraft.trim()
        val password = passwordDraft
        val deviceName = deviceDraft.trim().ifBlank { null } ?: deviceName()

        if (email.isBlank() || password.isBlank()) {
            showLoginError(getString(R.string.msg_sign_in_required_fields))
            return
        }

        showLoginError(null)
        signingIn = true
        loginButton?.isEnabled = false
        loginButton?.text = getString(R.string.btn_signing_in)
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
            signingIn = false
            loginButton?.isEnabled = true
            loginButton?.text = getString(R.string.btn_sign_in)

            if (error == null) {
                AccountEmailStore.save(this@MainActivity, email)
                passwordDraft = ""
                lastHomeKey = null
                configureToolbar()
                refreshUi()
            } else {
                // The API's messages (e.g. "Invalid email or password") carry no
                // trailing period.
                val detail = if (error.trimEnd().lastOrNull() in ".!?".toList()) error else "$error."
                showLoginError(getString(R.string.msg_sign_in_failed, detail))
            }
        }
    }

    // ---- Sign out, pause, resume -----------------------------------------

    private fun logout() {
        MaterialAlertDialogBuilder(this)
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
                    lastHomeKey = null
                    emailDraft = ""
                    passwordDraft = ""
                    configureToolbar()
                    refreshUi()
                }
            }
            .setNegativeButton(getString(R.string.dialog_cancel), null)
            .show()
    }

    private fun toggleMonitoring() {
        val isRunning = VirtueAccessibilityService.isEnabled()
        // Resuming needs Accessibility; without it the guide takes the place of
        // the question (CLIENT-011).
        if (!isRunning && !VirtueAccessibilityService.isConnected()) {
            refreshUi()
            return
        }
        val title = if (isRunning) getString(R.string.dialog_pause_title) else getString(R.string.dialog_resume_title)
        val message = if (isRunning) getString(R.string.dialog_pause_message) else getString(R.string.dialog_resume_message)
        val confirm = if (isRunning) getString(R.string.dialog_pause_confirm) else getString(R.string.dialog_resume_confirm)

        MaterialAlertDialogBuilder(this)
            .setTitle(title)
            .setMessage(message)
            .setPositiveButton(confirm) { _, _ ->
                if (isRunning) {
                    lifecycleScope.launch(Dispatchers.IO) {
                        NativeBridge.nativeNoteUserStop("android_pause_button")
                        VirtueAccessibilityService.pause()
                        withContext(Dispatchers.Main) { refreshUi() }
                    }
                } else {
                    VirtueAccessibilityService.resume()
                    binding.root.postDelayed({ refreshUi() }, 400)
                }
            }
            .setNegativeButton(getString(R.string.dialog_cancel), null)
            .show()
    }

    // ---- Setup Steps ------------------------------------------------------

    private fun renderSetupSteps() {
        val content = binding.content
        content.removeAllViews()
        val on = VirtueAccessibilityService.isConnected()
        content.addView(text(getString(if (on) R.string.setup_steps_on else R.string.setup_steps_off), 16f, R.color.ink_3).topMargin(8))
        val list = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
            ).apply { topMargin = dp(16) }
        }
        content.addView(list)
        AccessibilitySetupGuide.renderSteps(this, list, ::handleGuideAction)
    }

    private fun openAccessibilitySettings() {
        startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
    }

    private fun openAppInfo() {
        startActivity(
            Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS)
                .setData(Uri.fromParts("package", packageName, null))
        )
    }

    private fun openAppList() {
        startActivity(Intent(Settings.ACTION_MANAGE_APPLICATIONS_SETTINGS))
    }

    private fun handleGuideAction(action: AccessibilitySetupGuide.Action) {
        when (action) {
            AccessibilitySetupGuide.Action.ACCESSIBILITY_SETTINGS -> openAccessibilitySettings()
            AccessibilitySetupGuide.Action.APP_INFO -> openAppInfo()
            AccessibilitySetupGuide.Action.APP_LIST -> openAppList()
        }
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

    private fun deviceName(): String {
        val manufacturer = Build.MANUFACTURER.orEmpty()
        val model = Build.MODEL.orEmpty()
        return if (model.startsWith(manufacturer, ignoreCase = true)) model else "$manufacturer $model"
    }

    // ---- What Virtue Monitors --------------------------------------------

    private fun renderMonitors() {
        val content = binding.content
        content.removeAllViews()
        content.addView(text(getString(R.string.monitoring_summary), 16f).topMargin(8))
        listOf(
            R.string.monitors_every_app,
            R.string.monitors_blurred,
            R.string.monitors_locked,
            R.string.monitors_no_input,
            R.string.monitors_who_sees,
        ).forEach { content.addView(bullet(getString(it))) }
        content.addView(linkText(getString(R.string.monitoring_learn_more)) {
            startActivity(
                Intent(
                    Intent.ACTION_VIEW,
                    Uri.parse("https://virtueinitiative.org/help/what-virtue-monitors/android")
                )
            )
        }.topMargin(12))
    }

    private fun bullet(text: String): View = LinearLayout(this).apply {
        orientation = LinearLayout.HORIZONTAL
        layoutParams = LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
        ).apply { topMargin = dp(14) }
        addView(text("•", 16f, R.color.ink_3).apply {
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
            layoutParams = LinearLayout.LayoutParams(dp(20), ViewGroup.LayoutParams.WRAP_CONTENT)
        })
        addView(text(text, 16f, R.color.ink_2).apply {
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
        })
    }

    // ---- Status Details ---------------------------------------------------

    private fun renderStatusDetails() {
        // Every field here comes from the shared core status payload
        // (client/core/SPEC.md CORE-010), so this screen shows the same things
        // as the desktop clients', plus the Android-specific rows below.
        val json = statusJson()
        val state = currentState(json)
        val loggedIn = NativeBridge.nativeIsLoggedIn()
        val content = binding.content
        content.removeAllViews()

        // `optString` renders a JSON null as the literal "null", so absent and
        // null values are both routed through `isNull` first.
        fun optText(key: String): String =
            if (json.isNull(key)) "—" else json.optString(key, "").ifBlank { "—" }
        fun optTime(key: String): String = formatTimestampMs(json.optLong(key, 0))

        // Summary: dot, headline and one sentence.
        content.addView(LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
            ).apply { topMargin = dp(8) }
            addView(statusDot(tone(state)))
            addView(text(headline(state), 22f, bold = true).apply {
                ViewCompat.setAccessibilityHeading(this, true)
            })
        })
        content.addView(text(getString(
            when (state) {
                State.ACTIVE -> R.string.summary_active
                State.SIGNED_OUT -> R.string.summary_signed_out
                State.A11Y_OFF -> R.string.summary_a11y_off
                State.PAUSED, State.NOT_RUNNING -> R.string.summary_paused
                State.WAITING -> R.string.sentence_waiting
            }
        ), 16f, R.color.ink_3).topMargin(4))

        // Account
        val account = mutableListOf<DetailRow>()
        if (loggedIn) {
            account += DetailRow(
                Tone.GOOD, getString(R.string.details_signed_in), optText("account_email"),
                getString(R.string.details_device_hint, optText("device_name")),
            )
            val partners = if (json.isNull("partner_count")) null else json.optInt("partner_count", 0)
            account += DetailRow(
                if ((partners ?: 0) > 0) Tone.GOOD else Tone.WARN,
                getString(R.string.details_partners),
                partners?.toString() ?: "—",
                if (partners == 0) getString(R.string.details_partners_hint) else null,
            )
        } else {
            account += DetailRow(Tone.NEUTRAL, getString(R.string.details_account), getString(R.string.details_signed_out))
        }
        content.addView(sectionLabel(getString(R.string.details_group_account)))
        content.addView(detailGroup(account))

        // Android
        val connected = VirtueAccessibilityService.isConnected()
        val notificationsOn = NotificationManagerCompat.from(this).areNotificationsEnabled()
        val unrestricted = getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(packageName)
        content.addView(sectionLabel(getString(R.string.details_group_android)))
        content.addView(detailGroup(listOf(
            DetailRow(
                if (connected) Tone.GOOD else Tone.BAD,
                getString(R.string.details_accessibility),
                getString(if (connected) R.string.details_on else R.string.details_off),
                if (connected) null else getString(R.string.details_accessibility_hint),
            ),
            DetailRow(
                if (notificationsOn) Tone.GOOD else Tone.WARN,
                getString(R.string.details_notifications),
                getString(if (notificationsOn) R.string.details_on else R.string.details_off),
                if (notificationsOn) null else getString(R.string.details_notifications_hint),
            ),
            DetailRow(
                if (unrestricted) Tone.GOOD else Tone.WARN,
                getString(R.string.details_battery),
                getString(if (unrestricted) R.string.details_battery_unrestricted else R.string.details_battery_optimized),
                getString(if (unrestricted) R.string.details_battery_hint_ok else R.string.details_battery_hint_fix),
            ),
        )))

        // Screenshots
        val lastShot = lastScreenshotMs(json)
        val pendingHash = json.optInt("pending_hash_count", 0)
        val pendingBatch = json.optInt("pending_batch_count", 0)
        content.addView(sectionLabel(getString(R.string.details_group_screenshots)))
        content.addView(detailGroup(listOf(
            DetailRow(
                if (lastShot > 0) Tone.GOOD else if (loggedIn) Tone.WARN else Tone.NEUTRAL,
                getString(R.string.details_last_screenshot),
                if (lastShot > 0) formatClock(lastShot) else getString(R.string.details_none),
            ),
            DetailRow(null, getString(R.string.details_waiting_upload), (pendingHash + pendingBatch).toString()),
        )))

        // Problems: the newest error, if any.
        val recentErrors = json.optJSONArray("recent_errors")
        val latest = recentErrors?.optJSONObject(0)
        content.addView(sectionLabel(getString(R.string.details_group_problems)))
        content.addView(detailGroup(listOf(
            if (latest == null) {
                DetailRow(Tone.GOOD, getString(R.string.details_recent_problems), getString(R.string.details_none))
            } else {
                DetailRow(
                    Tone.BAD, getString(R.string.details_recent_problems), latest.optString("message"),
                    "${formatTimestampMs(latest.optLong("at_ms", 0))} · ${latest.optString("context")}",
                )
            }
        )))

        // Advanced
        val skipReason = when (if (json.isNull("last_skip_reason")) "" else json.optString("last_skip_reason", "")) {
            "static_screen" -> "Screen unchanged since the last upload"
            "locked_or_screensaver" -> "Screen locked or screensaver active"
            "capture_failed" -> "Capture failed"
            else -> "—"
        }
        val a11yStatus = when {
            VirtueAccessibilityService.isEnabled() -> "Active"
            VirtueAccessibilityService.isPaused() -> "Paused"
            VirtueAccessibilityService.isConnected() -> "Connected (not running)"
            else -> "Not enabled"
        }
        val hashBaseUrl = if (json.isNull("hash_base_url")) "default" else json.optString("hash_base_url", "").ifBlank { "default" }
        val logFile = java.io.File(filesDir, "core-data/logs").listFiles()?.maxByOrNull { it.lastModified() }

        val advancedBody = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            visibility = if (advancedOpen) View.VISIBLE else View.GONE
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
            )
        }
        advancedBody.addView(detailGroup(listOf(
            DetailRow(null, "Device ID", optText("device_id")),
            DetailRow(null, "API URL", optText("api_base_url")),
            DetailRow(null, "Hash base URL", hashBaseUrl),
            DetailRow(null, "Capture interval", "${json.optLong("capture_interval_seconds", 0)}s"),
            DetailRow(null, "Batch window", "${json.optLong("batch_window_seconds", 0)}s"),
            DetailRow(null, "Build", BuildConfig.VIRTUE_BUILD_LABEL),
            DetailRow(null, "Accessibility service", a11yStatus),
            DetailRow(null, "Waiting for hash", pendingHash.toString()),
            DetailRow(null, "Waiting in batch", pendingBatch.toString()),
            DetailRow(null, "Pending requests", json.optInt("pending_request_count", 0).toString()),
            DetailRow(null, "Last batch upload", optTime("last_batch_at_ms")),
            DetailRow(null, "Last loop", optTime("last_loop_at_ms")),
            DetailRow(null, "Last attempt", optTime("last_screenshot_attempt_at_ms")),
            DetailRow(null, "Last skip reason", skipReason),
            DetailRow(null, "Log file", logFile?.absolutePath ?: "—"),
        )))
        advancedBody.addView(tonalButton(getString(R.string.btn_open_log)) { openLogFile(logFile) }.topMargin(12))
        val test = tonalButton(getString(R.string.btn_take_test_screenshot)) { forceCapture() }
        test.isEnabled = loggedIn
        testButton = test
        advancedBody.addView(test.topMargin(8))
        val message = text(detailsMessage.orEmpty(), 14f, R.color.ink_3).apply {
            accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
            visibility = if (detailsMessage == null) View.GONE else View.VISIBLE
        }.topMargin(8) as TextView
        detailsMessageView = message
        advancedBody.addView(message)

        val chevron = ImageView(this).apply {
            setImageResource(R.drawable.ic_expand_more)
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
            rotation = if (advancedOpen) 180f else 0f
        }
        val header = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            minimumHeight = dp(56)
            isClickable = true
            isFocusable = true
            contentDescription = getString(R.string.details_advanced)
            addView(text(getString(R.string.details_advanced), 14f, R.color.ink_3, bold = true).apply {
                layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
            })
            addView(chevron)
            setOnClickListener {
                advancedOpen = !advancedOpen
                advancedBody.visibility = if (advancedOpen) View.VISIBLE else View.GONE
                chevron.rotation = if (advancedOpen) 180f else 0f
            }
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
            ).apply { topMargin = dp(12) }
        }
        content.addView(header)
        content.addView(advancedBody)
    }

    /** Shows [message] under the Advanced buttons, clearing it after 2.5 s when [clear] is set. */
    private fun showDetailsMessage(message: String, clear: Boolean) {
        detailsMessage = message
        detailsMessageView?.text = message
        detailsMessageView?.visibility = View.VISIBLE
        if (clear) {
            binding.root.postDelayed({
                if (detailsMessage == message) {
                    detailsMessage = null
                    detailsMessageView?.visibility = View.GONE
                }
            }, 2500)
        }
    }

    private fun forceCapture() {
        testButton?.isEnabled = false
        showDetailsMessage(getString(R.string.msg_force_capture_started), clear = false)
        lifecycleScope.launch {
            // The native call waits for the batch to land, so its message
            // reports what actually happened rather than assuming an upload.
            val raw = withContext(Dispatchers.IO) {
                NativeBridge.nativeForceCapture()
            }
            testButton?.isEnabled = true

            val report = runCatching { JSONObject(raw) }.getOrElse { JSONObject() }
            val error = if (report.isNull("error")) null else report.optString("error", "").ifBlank { null }
            val message = if (report.isNull("message")) "" else report.optString("message", "")
            if (error != null || message.isBlank()) {
                showDetailsMessage(getString(R.string.msg_force_capture_failed, error ?: raw), clear = true)
            } else {
                showDetailsMessage(message, clear = true)
            }
        }
    }

    // ---- Report an Issue --------------------------------------------------

    private fun renderReport() {
        val content = binding.content
        content.removeAllViews()

        val (messageLayout, message) = outlinedField(
            getString(R.string.report_bug_what_happened),
            InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES,
            EditorInfo.IME_FLAG_NO_ENTER_ACTION,
            null,
        )
        message.apply {
            setSingleLine(false)
            minLines = 4
            maxLines = 8
            gravity = Gravity.TOP or Gravity.START
        }
        messageLayout.topMargin(8)
        val (emailLayout, email) = outlinedField(
            getString(R.string.report_bug_contact_email_label),
            InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
            EditorInfo.IME_ACTION_DONE,
            View.AUTOFILL_HINT_EMAIL_ADDRESS,
        )
        email.setText(AccountEmailStore.load(this).orEmpty())

        val logs = MaterialCheckBox(this).apply {
            text = getString(R.string.report_bug_attach_logs)
            isChecked = true
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
            ).apply { topMargin = dp(12) }
        }
        val error = text("", 14f, R.color.bad).apply {
            visibility = View.GONE
            accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
        }.topMargin(8) as TextView
        val send = filledButton(getString(R.string.btn_send_report)) {}

        send.setOnClickListener {
            val body = message.text?.toString()?.trim().orEmpty()
            if (body.isEmpty()) {
                error.text = getString(R.string.report_bug_message_required)
                error.visibility = View.VISIBLE
                return@setOnClickListener
            }
            error.visibility = View.GONE
            val contactEmail = email.text?.toString()?.trim().orEmpty()
            send.isEnabled = false
            lifecycleScope.launch {
                val failure = withContext(Dispatchers.IO) {
                    NativeBridge.nativeReportIssue(body, contactEmail, logs.isChecked, androidPlatformDetails())
                }
                send.isEnabled = true
                if (failure == null) {
                    showScreen(Screen.HOME)
                    showReportSentDialog()
                } else {
                    error.text = getString(R.string.report_bug_send_failed)
                    error.visibility = View.VISIBLE
                }
            }
        }

        content.addView(messageLayout)
        content.addView(emailLayout)
        content.addView(logs)
        content.addView(text(getString(R.string.report_bug_attach_logs_caption), 13f, R.color.ink_3).apply {
            setPadding(dp(12), 0, 0, 0)
        })
        content.addView(error)
        content.addView(send.topMargin(20))
    }

    private fun showReportSentDialog() {
        MaterialAlertDialogBuilder(this)
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
            showDetailsMessage(getString(R.string.open_log_unavailable), clear = true)
            return
        }
        // Hand out a `.txt` copy: a viewer that sniffs the type from the file
        // name may not render `.log` (or older builds' extensionless files),
        // and a copy keeps it off a file the daemon is still appending to.
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
            showDetailsMessage(getString(R.string.open_log_no_viewer), clear = true)
        }
    }

    private fun formatTimestampMs(ms: Long): String {
        if (ms <= 0L) return "—"
        return SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.getDefault()).format(Date(ms))
    }
}
