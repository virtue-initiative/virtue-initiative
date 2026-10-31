package org.virtueinitiative.virtue

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import androidx.annotation.ColorRes
import androidx.annotation.DrawableRes
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.accessibility.AccessibilityNodeInfoCompat
import com.google.android.material.button.MaterialButton

/**
 * What a status dot or ring says. Color is never the only signal: each tone
 * has a spoken label (CLIENT-030, TalkBack).
 */
enum class Tone(@ColorRes val color: Int, val label: Int) {
    GOOD(R.color.good, R.string.tone_good),
    WARN(R.color.ochre, R.string.tone_warn),
    BAD(R.color.bad, R.string.tone_bad),
    NEUTRAL(R.color.ink_4, R.string.tone_neutral),
}

/** One row of Status Details: dot, label, value and an optional hint. */
data class DetailRow(val tone: Tone?, val label: String, val value: String, val hint: String? = null)

fun Context.dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

fun Context.color(@ColorRes id: Int): Int = ContextCompat.getColor(this, id)

fun Context.text(
    text: CharSequence,
    sizeSp: Float = 16f,
    @ColorRes color: Int = R.color.ink,
    bold: Boolean = false,
): TextView = TextView(this).apply {
    this.text = text
    setTextSize(TypedValue.COMPLEX_UNIT_SP, sizeSp)
    setTextColor(color(color))
    if (bold) typeface = Typeface.DEFAULT_BOLD
    layoutParams = LinearLayout.LayoutParams(
        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
    )
}

fun View.topMargin(dp: Int): View {
    (layoutParams as? ViewGroup.MarginLayoutParams)?.topMargin = context.dp(dp)
    return this
}

/** A link-looking line of text with a ripple, at least 48dp tall. */
fun Context.linkText(text: CharSequence, @ColorRes color: Int = R.color.virtue_link, onClick: () -> Unit): TextView =
    text(text, 15f, color).apply {
        minHeight = dp(48)
        gravity = Gravity.CENTER_VERTICAL
        isClickable = true
        isFocusable = true
        val outValue = TypedValue()
        context.theme.resolveAttribute(android.R.attr.selectableItemBackground, outValue, true)
        setBackgroundResource(outValue.resourceId)
        setOnClickListener { onClick() }
        ViewCompat.setAccessibilityDelegate(this, object : androidx.core.view.AccessibilityDelegateCompat() {
            override fun onInitializeAccessibilityNodeInfo(host: View, info: AccessibilityNodeInfoCompat) {
                super.onInitializeAccessibilityNodeInfo(host, info)
                info.roleDescription = context.getString(R.string.role_link)
            }
        })
    }

fun Context.filledButton(text: CharSequence, onClick: () -> Unit): MaterialButton =
    MaterialButton(this).apply {
        this.text = text
        minimumHeight = dp(52)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
        layoutParams = LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
        )
        setOnClickListener { onClick() }
    }

fun Context.tonalButton(text: CharSequence, onClick: () -> Unit): MaterialButton =
    filledButton(text, onClick).apply {
        backgroundTintList = ColorStateList.valueOf(color(R.color.bg_subtle))
        setTextColor(color(R.color.ink))
        rippleColor = ColorStateList.valueOf(color(R.color.border))
    }

fun Context.statusDot(tone: Tone): View = View(this).apply {
    background = GradientDrawable().apply {
        shape = GradientDrawable.OVAL
        setColor(color(tone.color))
    }
    contentDescription = getString(tone.label)
    layoutParams = LinearLayout.LayoutParams(dp(10), dp(10)).apply { marginEnd = dp(12) }
}

/** The 112dp home ring: a 6dp border in the state color around an icon. */
fun Context.statusRing(tone: Tone, @DrawableRes icon: Int, sizeDp: Int = 112, strokeDp: Int = 6): View =
    FrameLayout(this).apply {
        background = GradientDrawable().apply {
            shape = GradientDrawable.OVAL
            setColor(color(R.color.surface))
            setStroke(dp(strokeDp), color(tone.color))
        }
        layoutParams = LinearLayout.LayoutParams(dp(sizeDp), dp(sizeDp)).apply {
            gravity = Gravity.CENTER_HORIZONTAL
        }
        addView(ImageView(context).apply {
            setImageResource(icon)
            imageTintList = ColorStateList.valueOf(color(tone.color))
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
            layoutParams = FrameLayout.LayoutParams(dp(sizeDp / 2), dp(sizeDp / 2), Gravity.CENTER)
        })
        // The headline below carries the meaning; the ring only repeats it.
        importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
    }

fun Context.card(): LinearLayout = LinearLayout(this).apply {
    orientation = LinearLayout.VERTICAL
    setBackgroundResource(R.drawable.card_background)
    val pad = dp(20)
    setPadding(pad, pad, pad, pad)
    layoutParams = LinearLayout.LayoutParams(
        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
    )
}

fun Context.sectionLabel(title: String): TextView =
    text(title, 14f, R.color.ink_3, bold = true).apply {
        (layoutParams as LinearLayout.LayoutParams).topMargin = dp(20)
        (layoutParams as LinearLayout.LayoutParams).bottomMargin = dp(8)
        ViewCompat.setAccessibilityHeading(this, true)
    }

/** A bordered group of [DetailRow]s separated by hairlines. */
fun Context.detailGroup(rows: List<DetailRow>): LinearLayout = card().apply {
    setPadding(dp(16), dp(4), dp(16), dp(4))
    // Rows without a dot still line up with the ones that have one.
    val reserveDot = rows.any { it.tone != null }
    rows.forEachIndexed { index, row ->
        if (index > 0) {
            addView(View(context).apply {
                setBackgroundColor(color(R.color.border))
                layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(1))
            })
        }
        addView(detailRow(row, reserveDot))
    }
}

fun Context.detailRow(row: DetailRow, reserveDot: Boolean = false): View = LinearLayout(this).apply {
    orientation = LinearLayout.VERTICAL
    setPadding(0, dp(12), 0, dp(12))
    layoutParams = LinearLayout.LayoutParams(
        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
    )
    addView(LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        if (row.tone != null) addView(statusDot(row.tone))
        else if (reserveDot) addView(View(context).apply {
            layoutParams = LinearLayout.LayoutParams(dp(10), dp(10)).apply { marginEnd = dp(12) }
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        })
        addView(text(row.label, 15f, R.color.ink_3).apply {
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
        })
        addView(text(row.value.ifEmpty { "—" }, 15f, R.color.ink).apply {
            gravity = Gravity.END
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1.4f).apply {
                marginStart = dp(12)
            }
        })
    })
    if (row.hint != null) {
        addView(text(row.hint, 13f, R.color.ink_3).apply {
            (layoutParams as LinearLayout.LayoutParams).apply {
                topMargin = dp(4)
                if (row.tone != null || reserveDot) marginStart = dp(22)
            }
        })
    }
}

/** Progress segments: done in green, current in the accent color, the rest hollow. */
fun Context.progressBar(current: Int, total: Int): LinearLayout = LinearLayout(this).apply {
    orientation = LinearLayout.HORIZONTAL
    layoutParams = LinearLayout.LayoutParams(
        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
    )
    importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
    repeat(total) { index ->
        addView(View(context).apply {
            background = GradientDrawable().apply {
                cornerRadius = dp(2).toFloat()
                setColor(
                    color(
                        when {
                            index < current -> R.color.good
                            index == current -> R.color.virtue_forest
                            else -> R.color.border
                        }
                    )
                )
            }
            layoutParams = LinearLayout.LayoutParams(0, dp(4), 1f).apply {
                if (index > 0) marginStart = dp(6)
            }
        })
    }
}
