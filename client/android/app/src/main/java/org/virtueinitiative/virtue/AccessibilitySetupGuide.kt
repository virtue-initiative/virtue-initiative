package org.virtueinitiative.virtue

import android.content.Context
import android.content.pm.PackageInstaller
import android.content.res.ColorStateList
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.view.Gravity
import android.view.View
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import androidx.annotation.DrawableRes
import androidx.annotation.RequiresApi
import androidx.core.content.ContextCompat
import androidx.core.text.HtmlCompat
import com.google.android.material.button.MaterialButton
import com.google.android.material.imageview.ShapeableImageView
import com.google.android.material.shape.ShapeAppearanceModel

/**
 * Step-by-step guide for turning on Virtue's accessibility service.
 *
 * The wording and screenshots follow the running Android version, since the
 * Settings screens differ between releases. On Android 13+ an app installed
 * from a downloaded APK is blocked from Accessibility access ("restricted
 * settings") until the user allows it from App info, so the guide adds those
 * steps when that block applies.
 */
object AccessibilitySetupGuide {
    enum class Action { ACCESSIBILITY_SETTINGS, APP_INFO, APP_LIST }

    private data class Step(
        val title: String,
        val body: CharSequence,
        @DrawableRes val image: Int? = null,
        val action: Action? = null,
    )

    /**
     * Whether Android blocks Accessibility access until the user allows
     * restricted settings. That applies from Android 13 to apps installed from
     * a downloaded or local APK. Whether the user has since lifted the block
     * isn't readable by the app (the app-op behind it needs GET_APP_OPS_STATS),
     * so the guide keeps showing those steps until Accessibility is on.
     *
     * An in-app update ([AppUpdater]) resets the install source. Android 15+
     * lifts the block along with it, but Android 13 and 14 keep it, so there
     * the source first recorded by [recordInstallSource] counts too.
     */
    fun isRestricted(context: Context): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return false
        if (isSideloadSource(currentInstallSource(context))) return true
        return Build.VERSION.SDK_INT < Build.VERSION_CODES.VANILLA_ICE_CREAM &&
            isSideloadSource(installSourcePrefs(context).getInt(KEY_FIRST_SOURCE, -1))
    }

    /** Remembers the install source of the first launch, for [isRestricted]. */
    fun recordInstallSource(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
        val prefs = installSourcePrefs(context)
        if (prefs.contains(KEY_FIRST_SOURCE)) return
        val source = currentInstallSource(context) ?: return
        prefs.edit().putInt(KEY_FIRST_SOURCE, source).apply()
    }

    private const val KEY_FIRST_SOURCE = "first_package_source"

    private fun installSourcePrefs(context: Context) =
        context.getSharedPreferences("install_source", Context.MODE_PRIVATE)

    @RequiresApi(Build.VERSION_CODES.TIRAMISU)
    private fun currentInstallSource(context: Context): Int? = runCatching {
        context.packageManager.getInstallSourceInfo(context.packageName).packageSource
    }.getOrNull()

    @RequiresApi(Build.VERSION_CODES.TIRAMISU)
    private fun isSideloadSource(source: Int?) =
        source == PackageInstaller.PACKAGE_SOURCE_LOCAL_FILE ||
            source == PackageInstaller.PACKAGE_SOURCE_DOWNLOADED_FILE

    /** Fills [container] with the guide for this device's current state. */
    fun render(context: Context, container: LinearLayout, onAction: (Action) -> Unit) {
        container.removeAllViews()
        val restricted = isRestricted(context)

        container.addView(bodyText(context, context.getString(R.string.a11y_guide_intro)))
        if (restricted) {
            container.addView(bodyText(context, context.getString(R.string.a11y_guide_restricted_note)).apply {
                (layoutParams as LinearLayout.LayoutParams).topMargin = dp(context, 8)
            })
        }

        steps(context, restricted).forEachIndexed { index, step ->
            container.addView(stepView(context, index + 1, step, onAction))
        }
    }

    private fun steps(context: Context, restricted: Boolean): List<Step> {
        val sdk = Build.VERSION.SDK_INT
        val serviceLabel = context.getString(R.string.accessibility_service_label)
        val section = context.getString(
            if (sdk >= Build.VERSION_CODES.S) R.string.a11y_section_downloaded_apps
            else R.string.a11y_section_downloaded_services
        )
        val toggleLabel = if (sdk >= Build.VERSION_CODES.S) {
            context.getString(R.string.a11y_toggle_use_service_named, serviceLabel)
        } else {
            context.getString(R.string.a11y_toggle_use_service)
        }

        val allow = Step(
            title = context.getString(R.string.a11y_step_allow_title),
            body = html(context.getString(R.string.a11y_step_allow_body)),
            image = allowImage(),
        )
        val back = Step(
            title = context.getString(R.string.a11y_step_return_title),
            body = html(context.getString(R.string.a11y_step_return_body)),
        )

        if (!restricted) {
            return listOf(
                Step(
                    title = context.getString(R.string.a11y_step_open_title),
                    body = html(context.getString(R.string.a11y_step_open_body, section, serviceLabel)),
                    image = listImage(),
                    action = Action.ACCESSIBILITY_SETTINGS,
                ),
                Step(
                    title = context.getString(R.string.a11y_step_toggle_title),
                    body = html(context.getString(R.string.a11y_step_toggle_body, toggleLabel)),
                    image = toggleImage(),
                ),
                allow,
                back,
            )
        }

        val warningTitle = context.getString(
            if (sdk >= Build.VERSION_CODES.VANILLA_ICE_CREAM) R.string.a11y_warning_denied
            else R.string.a11y_warning_restricted
        )
        val warningButton = context.getString(
            if (sdk >= 36) R.string.a11y_warning_button_close else R.string.a11y_warning_button_ok
        )

        return listOf(
            Step(
                title = context.getString(R.string.a11y_step_open_title),
                body = html(context.getString(R.string.a11y_step_open_body_restricted, section, serviceLabel)),
                image = listImage(),
                action = Action.ACCESSIBILITY_SETTINGS,
            ),
            Step(
                title = context.getString(R.string.a11y_step_warning_title),
                body = html(context.getString(R.string.a11y_step_warning_body, warningTitle, warningButton)),
                image = warningImage(),
            ),
            // Android 13 only offers "Allow restricted settings" when App info
            // is reached from Settings' own app list, not when another app
            // opens it directly, so send the user to the list there.
            if (sdk >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                Step(
                    title = context.getString(R.string.a11y_step_unlock_title),
                    body = html(context.getString(R.string.a11y_step_unlock_body)),
                    image = menuImage(),
                    action = Action.APP_INFO,
                )
            } else {
                Step(
                    title = context.getString(R.string.a11y_step_unlock_title),
                    body = html(context.getString(R.string.a11y_step_unlock_body_from_list)),
                    image = menuImage(),
                    action = Action.APP_LIST,
                )
            },
            Step(
                title = context.getString(R.string.a11y_step_toggle_title),
                body = html(context.getString(R.string.a11y_step_toggle_body_restricted, serviceLabel, toggleLabel)),
                image = toggleImage(),
                action = Action.ACCESSIBILITY_SETTINGS,
            ),
            allow,
            back,
        )
    }

    // Screenshots are grouped by the Settings design they show: Android 10–11,
    // 12, 13–14, 15–16 and 17+.
    private fun listImage() = when {
        Build.VERSION.SDK_INT >= 37 -> R.drawable.a11y_list_v17
        Build.VERSION.SDK_INT >= 35 -> R.drawable.a11y_list_v15
        Build.VERSION.SDK_INT >= 33 -> R.drawable.a11y_list_v13
        Build.VERSION.SDK_INT >= 31 -> R.drawable.a11y_list_v12
        else -> R.drawable.a11y_list_v10
    }

    private fun warningImage() = when {
        Build.VERSION.SDK_INT >= 37 -> R.drawable.a11y_restricted_v17
        Build.VERSION.SDK_INT >= 36 -> R.drawable.a11y_restricted_v16
        Build.VERSION.SDK_INT >= 35 -> R.drawable.a11y_restricted_v15
        else -> R.drawable.a11y_restricted_v13
    }

    private fun menuImage() = when {
        Build.VERSION.SDK_INT >= 37 -> R.drawable.a11y_menu_v17
        Build.VERSION.SDK_INT >= 35 -> R.drawable.a11y_menu_v15
        else -> R.drawable.a11y_menu_v13
    }

    private fun toggleImage() = when {
        Build.VERSION.SDK_INT >= 37 -> R.drawable.a11y_toggle_v17
        Build.VERSION.SDK_INT >= 35 -> R.drawable.a11y_toggle_v15
        Build.VERSION.SDK_INT >= 33 -> R.drawable.a11y_toggle_v13
        Build.VERSION.SDK_INT >= 31 -> R.drawable.a11y_toggle_v12
        else -> R.drawable.a11y_toggle_v10
    }

    private fun allowImage() = when {
        Build.VERSION.SDK_INT >= 37 -> R.drawable.a11y_allow_v17
        Build.VERSION.SDK_INT >= 35 -> R.drawable.a11y_allow_v15
        Build.VERSION.SDK_INT >= 33 -> R.drawable.a11y_allow_v13
        Build.VERSION.SDK_INT >= 31 -> R.drawable.a11y_allow_v12
        else -> R.drawable.a11y_allow_v10
    }

    private fun stepView(context: Context, number: Int, step: Step, onAction: (Action) -> Unit): View {
        val forest = ContextCompat.getColor(context, R.color.virtue_forest)
        val surface = ContextCompat.getColor(context, R.color.surface)
        val border = ContextCompat.getColor(context, R.color.border)

        val row = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT
            ).apply { topMargin = dp(context, 18) }
        }

        row.addView(TextView(context).apply {
            text = number.toString()
            gravity = Gravity.CENTER
            textSize = 13f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(surface)
            background = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                setColor(forest)
            }
            layoutParams = LinearLayout.LayoutParams(dp(context, 26), dp(context, 26)).apply {
                marginEnd = dp(context, 12)
            }
        })

        val column = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
        }
        row.addView(column)

        column.addView(TextView(context).apply {
            text = step.title
            textSize = 15f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(ContextCompat.getColor(context, R.color.ink))
            minHeight = dp(context, 26)
            gravity = Gravity.CENTER_VERTICAL
        })

        column.addView(bodyText(context, step.body).apply {
            (layoutParams as LinearLayout.LayoutParams).topMargin = dp(context, 2)
        })

        step.image?.let { image ->
            column.addView(ShapeableImageView(context).apply {
                setImageResource(image)
                adjustViewBounds = true
                scaleType = ImageView.ScaleType.FIT_CENTER
                maxHeight = dp(context, 300)
                contentDescription = context.getString(R.string.a11y_screenshot_description, step.title)
                shapeAppearanceModel = ShapeAppearanceModel.builder()
                    .setAllCornerSizes(dp(context, 6).toFloat())
                    .build()
                strokeColor = ColorStateList.valueOf(border)
                strokeWidth = dp(context, 1).toFloat()
                val inset = dp(context, 1) / 2
                setPadding(inset, inset, inset, inset)
                layoutParams = LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT
                ).apply { topMargin = dp(context, 10) }
            })
        }

        step.action?.let { action ->
            column.addView(MaterialButton(context).apply {
                text = context.getString(
                    when (action) {
                        Action.ACCESSIBILITY_SETTINGS -> R.string.btn_open_accessibility_settings
                        Action.APP_INFO -> R.string.btn_open_app_info
                        Action.APP_LIST -> R.string.btn_open_app_list
                    }
                )
                backgroundTintList = ColorStateList.valueOf(forest)
                setOnClickListener { onAction(action) }
                layoutParams = LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT
                ).apply { topMargin = dp(context, 8) }
            })
        }

        return row
    }

    private fun bodyText(context: Context, text: CharSequence) = TextView(context).apply {
        this.text = text
        textSize = 14f
        setLineSpacing(dp(context, 2).toFloat(), 1f)
        setTextColor(ContextCompat.getColor(context, R.color.ink_2))
        layoutParams = LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT
        )
    }

    private fun html(source: String): CharSequence =
        HtmlCompat.fromHtml(source, HtmlCompat.FROM_HTML_MODE_LEGACY)

    private fun dp(context: Context, value: Int): Int =
        (value * context.resources.displayMetrics.density).toInt()
}
