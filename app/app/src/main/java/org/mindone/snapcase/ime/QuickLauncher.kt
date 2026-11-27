// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.view.KeyCharacterMap
import android.view.KeyEvent
import java.text.Normalizer
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
    val activityName: String = "",
)

object QuickLauncher {
    private val combiningMarks = Regex("\\p{InCombiningDiacriticalMarks}+")

    private val genericPkgSegments: Set<String> = setOf(
        "com", "org", "net", "io", "co", "jp", "kr", "cn", "tw", "ru", "de", "fr", "uk", "edu", "gov",
        "android", "app", "apps", "mobile", "client", "main", "google", "samsung", "sec", "miui",
    )

    private val qwertyToRu: Map<Char, Char> = mapOf(
        'q' to 'й', 'w' to 'ц', 'e' to 'у', 'r' to 'к', 't' to 'е', 'y' to 'н', 'u' to 'г', 'i' to 'ш', 'o' to 'щ', 'p' to 'з',
        '[' to 'х', ']' to 'ъ', 'a' to 'ф', 's' to 'ы', 'd' to 'в', 'f' to 'а', 'g' to 'п', 'h' to 'р', 'j' to 'о', 'k' to 'л',
        'l' to 'д', ';' to 'ж', '\'' to 'э', 'z' to 'я', 'x' to 'ч', 'c' to 'с', 'v' to 'м', 'b' to 'и', 'n' to 'т', 'm' to 'ь',
        ',' to 'б', '.' to 'ю', '`' to 'ё',
    )

    private val ruToQwerty: Map<Char, Char> = qwertyToRu.entries.associate { (k, v) -> v to k } + mapOf(
        'і' to 's', 'ї' to ']', 'є' to '\'', 'ґ' to '`', 'е' to 't',
    )

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

    fun launch(context: Context, packageName: String, activityName: String = ""): Boolean {
        val intent = if (activityName.isNotBlank()) {
            Intent(Intent.ACTION_MAIN).apply {
                addCategory(Intent.CATEGORY_LAUNCHER)
                component = ComponentName(packageName, activityName)
            }
        } else {
            context.packageManager.getLaunchIntentForPackage(packageName)
        } ?: context.packageManager.getLaunchIntentForPackage(packageName) ?: return false
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED)
        return runCatching {
            context.startActivity(intent)
            true
        }.getOrDefault(false)
    }

    fun launch(context: Context, app: LaunchableApp): Boolean =
        launch(context, app.packageName, app.activityName)

    fun listLaunchableApps(context: Context): List<LaunchableApp> {
        val pm = context.packageManager
        val main = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        return pm.queryIntentActivities(main, 0)
            .mapNotNull { info ->
                val activity = info.activityInfo ?: return@mapNotNull null
                val pkg = activity.packageName ?: return@mapNotNull null
                val label = info.loadLabel(pm).toString().takeIf { it.isNotBlank() } ?: pkg
                LaunchableApp(pkg, label, activity.name.orEmpty())
            }
            .distinctBy { "${it.packageName}/${it.activityName}" }
            .sortedBy { it.label.lowercase() }
    }

    fun keyLetter(keyCode: Int): String {
        if (keyCode in KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z) {
            return ('A' + (keyCode - KeyEvent.KEYCODE_A)).toString()
        }
        return KeyEvent.keyCodeToString(keyCode).removePrefix("KEYCODE_")
    }

    fun extractSearchChar(keyCode: Int, unicodeChar: Int, shifted: Boolean): Char? {
        if (unicodeChar != 0 && (unicodeChar and KeyCharacterMap.COMBINING_ACCENT) == 0) {
            val ch = (unicodeChar and KeyCharacterMap.COMBINING_ACCENT_MASK).toChar()
            if (Character.isLetterOrDigit(ch)) return ch
        }
        if (keyCode in KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z) {
            val base = 'a' + (keyCode - KeyEvent.KEYCODE_A)
            return if (shifted) base.uppercaseChar() else base
        }
        if (keyCode in KeyEvent.KEYCODE_0..KeyEvent.KEYCODE_9 && !shifted) {
            return '0' + (keyCode - KeyEvent.KEYCODE_0)
        }
        if (keyCode in KeyEvent.KEYCODE_NUMPAD_0..KeyEvent.KEYCODE_NUMPAD_9) {
            return '0' + (keyCode - KeyEvent.KEYCODE_NUMPAD_0)
        }
        return null
    }

    fun isHomeLauncherPackage(
        rootPkg: String?,
        defaultHomePkg: String?,
        installedHomePkgs: Set<String>,
    ): Boolean {
        if (rootPkg.isNullOrBlank()) return false
        if (!defaultHomePkg.isNullOrBlank() && defaultHomePkg != "android") {
            return rootPkg == defaultHomePkg
        }
        return rootPkg in installedHomePkgs && rootPkg != "android" && rootPkg != "com.android.settings"
    }

    fun foldForSearch(text: String): String {
        val sb = StringBuilder(text.length)
        for (raw in text.trim().lowercase()) {
            if (HangulComposer.isHangul(raw)) {
                sb.append(raw)
                continue
            }
            val decomposed = Normalizer.normalize(raw.toString(), Normalizer.Form.NFD)
            val stripped = combiningMarks.replace(decomposed, "")
            for (c in stripped) {
                when (c) {
                    'ё' -> sb.append('е')
                    'ß' -> sb.append("ss")
                    'ł' -> sb.append('l')
                    'ø' -> sb.append('o')
                    'đ' -> sb.append('d')
                    'ı' -> sb.append('i')
                    'æ' -> sb.append("ae")
                    'œ' -> sb.append("oe")
                    else -> sb.append(c)
                }
            }
        }
        return sb.toString()
    }

    fun alternateLayoutQueries(folded: String): List<String> {
        if (folded.isEmpty()) return emptyList()
        val toLat = mapWithTable(folded, ruToQwerty)
        val toCyr = mapWithTable(folded, qwertyToRu)
        val toKo = if (folded.all { it in 'a'..'z' }) HangulComposer.composeFromLatin(folded) else ""
        return listOf(toLat, toCyr, toKo).filter { it.isNotEmpty() && it != folded }.distinct()
    }

    private fun mapWithTable(text: String, table: Map<Char, Char>): String {
        val sb = StringBuilder(text.length)
        for (c in text) {
            sb.append(table[c] ?: c)
        }
        return sb.toString()
    }

    fun searchApps(
        apps: List<LaunchableApp>,
        rawQuery: String,
        recentPackages: List<String> = emptyList(),
    ): List<LaunchableApp> {
        val recentRank = HashMap<String, Int>(recentPackages.size)
        recentPackages.forEachIndexed { idx, pkg -> recentRank.putIfAbsent(pkg, idx) }
        val q = foldForSearch(rawQuery)
        if (q.isEmpty()) {
            return apps.sortedWith(
                compareBy<LaunchableApp> { recentRank[it.packageName] ?: Int.MAX_VALUE }
                    .thenBy { it.label.lowercase() },
            )
        }
        val alternates = alternateLayoutQueries(q)
        val scored = ArrayList<Pair<Int, LaunchableApp>>(apps.size)
        for (app in apps) {
            val labelFold = foldForSearch(app.label)
            val words = splitSearchWords(app.label)
            val pkgSegments = packageSearchSegments(app.packageName)
            var tier = matchTier(labelFold, words, pkgSegments, q)
            if (tier < 0) {
                var bestAlt = Int.MAX_VALUE
                for (alt in alternates) {
                    val altTier = matchTier(labelFold, words, pkgSegments, alt)
                    if (altTier >= 0 && altTier < bestAlt) bestAlt = altTier
                }
                if (bestAlt != Int.MAX_VALUE) tier = bestAlt + 6
            }
            if (tier >= 0) {
                scored.add(tier to app)
            }
        }
        scored.sortWith(
            compareBy<Pair<Int, LaunchableApp>> { it.first }
                .thenBy { recentRank[it.second.packageName] ?: Int.MAX_VALUE }
                .thenBy { it.second.label.length }
                .thenBy { it.second.label.lowercase() },
        )
        return scored.map { it.second }
    }

    private fun packageSearchSegments(packageName: String): List<String> =
        packageName.lowercase()
            .split('.', '_', '-')
            .filter { it.length >= 3 && it !in genericPkgSegments }

    private fun matchTier(
        labelFold: String,
        words: List<String>,
        pkgSegments: List<String>,
        q: String,
    ): Int {
        if (labelFold == q) return 0
        if (labelFold.startsWith(q)) return 1
        if (words.any { it.startsWith(q) }) return 2
        if (q.length >= 2 && matchesAcronym(words, q)) return 3
        if (labelFold.contains(q)) return 4
        if (q.length >= 3 && pkgSegments.any { it.startsWith(q) }) return 5
        return -1
    }

    private fun splitSearchWords(label: String): List<String> {
        val words = ArrayList<String>(4)
        val current = StringBuilder()
        var prevLower = false
        for (ch in label) {
            if (!Character.isLetterOrDigit(ch)) {
                if (current.isNotEmpty()) {
                    words.add(foldForSearch(current.toString()))
                    current.setLength(0)
                }
                prevLower = false
                continue
            }
            if (prevLower && Character.isUpperCase(ch) && current.isNotEmpty()) {
                words.add(foldForSearch(current.toString()))
                current.setLength(0)
            }
            current.append(ch)
            prevLower = Character.isLowerCase(ch)
        }
        if (current.isNotEmpty()) {
            words.add(foldForSearch(current.toString()))
        }
        return words.filter { it.isNotEmpty() }
    }

    private fun matchesAcronym(words: List<String>, q: String): Boolean {
        if (words.size < 2) return false
        var wi = 0
        for (ch in q) {
            var found = false
            while (wi < words.size) {
                if (words[wi].firstOrNull() == ch) {
                    wi++
                    found = true
                    break
                }
                wi++
            }
            if (!found) return false
        }
        return true
    }
}
