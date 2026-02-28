package codes.anb.virtue

import android.content.Context
import android.content.Intent
import android.os.Parcel
import android.util.Base64

object ProjectionPermissionStore {
    private const val PREFS = "virtue_projection"
    private const val KEY_RESULT_CODE = "result_code"
    private const val KEY_INTENT_B64 = "intent_b64"

    fun save(context: Context, resultCode: Int, data: Intent) {
        val parcel = Parcel.obtain()
        data.writeToParcel(parcel, 0)
        val bytes = parcel.marshall()
        parcel.recycle()

        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .putInt(KEY_RESULT_CODE, resultCode)
            .putString(KEY_INTENT_B64, Base64.encodeToString(bytes, Base64.NO_WRAP))
            .apply()
    }

    fun load(context: Context): Pair<Int, Intent>? {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val code = prefs.getInt(KEY_RESULT_CODE, Int.MIN_VALUE)
        val b64 = prefs.getString(KEY_INTENT_B64, null) ?: return null
        if (code == Int.MIN_VALUE) return null

        return try {
            val bytes = Base64.decode(b64, Base64.NO_WRAP)
            val parcel = Parcel.obtain()
            parcel.unmarshall(bytes, 0, bytes.size)
            parcel.setDataPosition(0)
            val intent = Intent.CREATOR.createFromParcel(parcel)
            parcel.recycle()
            code to intent
        } catch (_: Throwable) {
            null
        }
    }

    fun clear(context: Context) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .clear()
            .apply()
    }
}
