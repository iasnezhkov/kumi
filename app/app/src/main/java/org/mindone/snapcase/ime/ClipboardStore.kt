// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import android.content.ClipboardManager
import android.content.Context
import java.io.File
import java.util.concurrent.Executors
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONArray
import org.json.JSONObject

data class ClipboardItem(
    val text: String,
    val timestampMs: Long,
    val pinned: Boolean = false,
)

data class TextSnippet(
    val trigger: String,
    val expansion: String,
) {
    val valid: Boolean get() = trigger.isNotBlank() && expansion.isNotEmpty()
}

class ClipboardStore(private val file: File) {
    private val _items = MutableStateFlow<List<ClipboardItem>>(emptyList())
    val items: StateFlow<List<ClipboardItem>> = _items.asStateFlow()
    private val writer = Executors.newSingleThreadExecutor { r -> Thread(r, "kumi-clip-io").apply { isDaemon = true } }

    init {
        load()
    }

    private fun pruneExpired(list: List<ClipboardItem>, nowMs: Long): List<ClipboardItem> =
        list.filter { it.pinned || (nowMs - it.timestampMs) in 0L..UNPINNED_TTL_MS }

    fun record(raw: String, nowMs: Long = System.currentTimeMillis()) {
        val text = raw.trim()
        if (text.isEmpty() || text.length > MAX_ITEM_CHARS) return
        val current = pruneExpired(_items.value, nowMs)
        val existing = current.firstOrNull { it.text == text }
        val updated = ClipboardItem(text, nowMs, pinned = existing?.pinned == true)
        val rest = current.filterNot { it.text == text }
        val pinned = (listOf(updated).filter { it.pinned } + rest.filter { it.pinned }).take(MAX_ITEMS)
        val unpinned = (listOf(updated).filterNot { it.pinned } + rest.filterNot { it.pinned })
            .take((MAX_ITEMS - pinned.size).coerceAtLeast(0))
        _items.value = pinned + unpinned
        if (updated.pinned) save()
    }

    fun togglePin(text: String) {
        val nowMs = System.currentTimeMillis()
        val updated = pruneExpired(_items.value, nowMs).map { if (it.text == text) it.copy(pinned = !it.pinned) else it }
        _items.value = updated.filter { it.pinned } + updated.filterNot { it.pinned }
        save()
    }

    fun remove(text: String) {
        val removedPinned = _items.value.any { it.text == text && it.pinned }
        _items.value = pruneExpired(_items.value.filterNot { it.text == text }, System.currentTimeMillis())
        if (removedPinned) save()
    }

    fun clearUnpinned() {
        _items.value = _items.value.filter { it.pinned }
    }

    fun attachSystemClipboard(context: Context): () -> Unit {
        val manager = context.getSystemService(ClipboardManager::class.java) ?: return {}
        val readCurrent = {
            val clip = runCatching { manager.primaryClip }.getOrNull()
            val desc = clip?.description
            val sensitive = desc?.extras?.getBoolean("android.content.extra.IS_SENSITIVE", false) == true
            if (!sensitive && clip != null && clip.itemCount > 0) {
                clip.getItemAt(0)?.coerceToText(context)?.toString()?.let { record(it) }
            }
        }
        readCurrent()
        val listener = ClipboardManager.OnPrimaryClipChangedListener { readCurrent() }
        manager.addPrimaryClipChangedListener(listener)
        return { runCatching { manager.removePrimaryClipChangedListener(listener) } }
    }

    private fun load() {
        if (!file.isFile) return
        val parsed = runCatching {
            val array = JSONArray(file.readText())
            (0 until array.length()).mapNotNull { i ->
                val obj = array.optJSONObject(i) ?: return@mapNotNull null
                val text = obj.optString("t").takeIf { it.isNotEmpty() } ?: return@mapNotNull null
                val pinned = obj.optBoolean("p", false)
                if (!pinned) return@mapNotNull null
                ClipboardItem(text, obj.optLong("ts", 0L), true)
            }
        }.getOrDefault(emptyList())
        _items.value = parsed.take(MAX_ITEMS)
    }

    private fun save() {
        val snapshot = _items.value.filter { it.pinned }
        writer.execute {
            runCatching {
                file.parentFile?.mkdirs()
                val array = JSONArray()
                snapshot.forEach { item ->
                    array.put(JSONObject().put("t", item.text).put("ts", item.timestampMs).put("p", true))
                }
                val tmp = File(file.parentFile, "${file.name}.tmp")
                tmp.writeText(array.toString())
                if (!tmp.renameTo(file)) {
                    file.writeText(array.toString())
                    tmp.delete()
                }
            }
        }
    }

    companion object {
        const val MAX_ITEMS = 20
        const val MAX_ITEM_CHARS = 2_000
        const val UNPINNED_TTL_MS = 3_600_000L

        fun defaultFile(context: Context) = File(File(context.filesDir, "ime"), "clipboard.json")

        fun parseSnippets(raw: String?): List<TextSnippet> {
            if (raw.isNullOrBlank()) return DEFAULT_SNIPPETS
            return runCatching {
                val arr = JSONArray(raw)
                (0 until arr.length()).mapNotNull { i ->
                    val o = arr.optJSONObject(i) ?: return@mapNotNull null
                    TextSnippet(o.optString("k"), o.optString("v")).takeIf { it.valid }
                }
            }.getOrDefault(DEFAULT_SNIPPETS)
        }

        fun serializeSnippets(list: List<TextSnippet>): String {
            val arr = JSONArray()
            list.filter { it.valid }.forEach { arr.put(JSONObject().put("k", it.trigger).put("v", it.expansion)) }
            return arr.toString()
        }

        val DEFAULT_SNIPPETS: List<TextSnippet> = listOf(
            TextSnippet("omw", "On my way!"),
            TextSnippet("shrug", "¯\\_(ツ)_/¯"),
            TextSnippet("спс", "Спасибо!"),
        )
    }
}
