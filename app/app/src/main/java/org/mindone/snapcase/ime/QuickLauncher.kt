// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import android.content.Context
import android.content.Intent
import android.view.KeyEvent
import org.json.JSONArray
import org.json.JSONObject

data class SpeedKeyBinding(
    val keyCode: Int,
    val packageName: String,
    val label: String,
) {
    val valid: Boolean get() = keyCode in KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z && packageName.isNotBlank()
}

data class LaunchableApp(
    val packageName: String,
    val label: String,
)

object QuickLauncher {
    fun parseBindings(raw: String?): List<SpeedKeyBinding> {
        if (raw.isNullOrBlank()) return emptyList()
        return runCatching {
            val arr = JSONArray(raw)
            (0 until arr.length()).mapNotNull { i ->
                val o = arr.optJSONObject(i) ?: return@mapNotNull null
                SpeedKeyBinding(o.optInt("k"), o.optString("p"), o.optString("l")).takeIf { it.valid }
            }
        }.getOrDefault(emptyList())
    }

    fun serializeBindings(list: List<SpeedKeyBinding>): String {
        val arr = JSONArray()
        list.filter { it.valid }.forEach { b ->
            arr.put(JSONObject().put("k", b.keyCode).put("p", b.packageName).put("l", b.label))
        }
        return arr.toString()
    }

    fun match(keyCode: Int, bindings: List<SpeedKeyBinding>): SpeedKeyBinding? =
        bindings.firstOrNull { it.keyCode == keyCode && it.valid }

    fun launch(context: Context, packageName: String): Boolean {
        val intent = context.packageManager.getLaunchIntentForPackage(packageName) ?: return false
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED)
        return runCatching {
            context.startActivity(intent)
            true
        }.getOrDefault(false)
    }

    fun listLaunchableApps(context: Context): List<LaunchableApp> {
        val pm = context.packageManager
        val main = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        return pm.queryIntentActivities(main, 0)
            .mapNotNull { info ->
                val pkg = info.activityInfo?.packageName ?: return@mapNotNull null
                val label = info.loadLabel(pm)?.toString()?.takeIf { it.isNotBlank() } ?: pkg
                LaunchableApp(pkg, label)
            }
            .distinctBy { it.packageName }
            .sortedBy { it.label.lowercase() }
    }

    fun keyLetter(keyCode: Int): String {
        if (keyCode in KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z) {
            return ('A' + (keyCode - KeyEvent.KEYCODE_A)).toString()
        }
        return KeyEvent.keyCodeToString(keyCode).removePrefix("KEYCODE_")
    }
}
