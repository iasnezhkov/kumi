// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import android.content.Context
import java.io.File

data class EmojiEntry(
    val glyph: String,
    val category: String,
    val keywords: String,
)

class EmojiCatalog(private val context: Context) {
    private val recentsFile = File(File(context.filesDir, "ime"), "recent_emoji.txt")
    val entries: List<EmojiEntry> by lazy { loadAssets() }
    private val recents = ArrayList<String>()

    init {
        loadRecents()
    }

    fun recentGlyphs(): List<String> = synchronized(recents) { recents.toList() }

    fun recordRecent(glyph: String) {
        synchronized(recents) {
            recents.remove(glyph)
            recents.add(0, glyph)
            while (recents.size > MAX_RECENTS) recents.removeAt(recents.lastIndex)
            runCatching {
                recentsFile.parentFile?.mkdirs()
                recentsFile.writeText(recents.joinToString("\n"))
            }
        }
    }

    fun search(query: String, category: String? = null): List<EmojiEntry> {
        val q = query.trim().lowercase()
        if (q.isEmpty()) {
            return if (category == null || category == "all") entries else entries.filter { it.category == category }
        }
        return entries.filter { it.keywords.contains(q) }
    }

    private fun loadAssets(): List<EmojiEntry> = runCatching {
        context.assets.open("dict/emoji.tsv").bufferedReader(Charsets.UTF_8).useLines { lines ->
            lines.mapNotNull { line ->
                val parts = line.split('\t')
                if (parts.size < 3) null else EmojiEntry(parts[0], parts[1], parts[2].lowercase())
            }.toList()
        }
    }.getOrDefault(emptyList())

    private fun loadRecents() {
        if (!recentsFile.isFile) return
        runCatching {
            synchronized(recents) {
                recents.clear()
                recentsFile.readLines().filter { it.isNotBlank() }.take(MAX_RECENTS).forEach { recents.add(it) }
            }
        }
    }

    companion object {
        private const val MAX_RECENTS = 24
        val CATEGORIES: List<Pair<String, String>> = listOf(
            "recent" to "🕘",
            "smileys" to "😀",
            "gestures" to "👍",
            "hearts" to "❤️",
            "nature" to "🔥",
            "objects" to "🎧",
            "symbols" to "✅",
        )
    }
}
