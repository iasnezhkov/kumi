// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime.dict

import java.io.File
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors

class PersonalDictionary(private val dir: File) {
    private val cache = ConcurrentHashMap<String, LinkedHashMap<String, Int>>()
    private val bigramCache = ConcurrentHashMap<String, LinkedHashMap<String, LinkedHashMap<String, Int>>>()
    private val writer = Executors.newSingleThreadExecutor { r -> Thread(r, "kumi-dict-io").apply { isDaemon = true } }

    private fun fileFor(lang: String): File = File(dir, "personal_${lang.lowercase()}.tsv")

    private fun bigramFileFor(lang: String): File = File(dir, "personal_bigrams_${lang.lowercase()}.tsv")

    private fun mapFor(lang: String): LinkedHashMap<String, Int> = cache.computeIfAbsent(lang.lowercase()) {
        val loaded = LinkedHashMap<String, Int>()
        val file = fileFor(it)
        if (file.isFile) {
            runCatching {
                file.forEachLine { line ->
                    val parts = line.split('\t')
                    if (parts.size >= 2) {
                        val word = parts[0].trim()
                        val count = parts[1].trim().toIntOrNull() ?: 1
                        if (word.length in 2..32 && count > 0) loaded[word] = count
                    }
                }
            }
        }
        loaded
    }

    private fun bigramsFor(lang: String): LinkedHashMap<String, LinkedHashMap<String, Int>> =
        bigramCache.computeIfAbsent(lang.lowercase()) {
            val loaded = LinkedHashMap<String, LinkedHashMap<String, Int>>()
            val file = bigramFileFor(it)
            if (file.isFile) {
                runCatching {
                    file.forEachLine { line ->
                        val parts = line.split('\t')
                        if (parts.size >= 3) {
                            val prev = parts[0].trim()
                            val next = parts[1].trim()
                            val count = parts[2].trim().toIntOrNull() ?: 1
                            if (prev.isNotEmpty() && next.isNotEmpty() && count > 0) {
                                loaded.getOrPut(prev) { LinkedHashMap() }[next] = count
                            }
                        }
                    }
                }
            }
            loaded
        }

    fun learn(lang: String, word: String) {
        val clean = word.trim()
        if (clean.length !in 2..32 || clean.any { it.isWhitespace() || it.isDigit() }) return
        val map = mapFor(lang)
        synchronized(map) {
            val next = (map[clean] ?: 0) + 1
            map[clean] = next.coerceAtMost(10_000)
            if (map.size > MAX_WORDS) {
                val oldest = map.entries.minByOrNull { it.value }?.key
                if (oldest != null) map.remove(oldest)
            }
            saveLocked(lang, map)
        }
    }

    fun learnBigram(lang: String, prevWord: String, nextWord: String) {
        val prev = if (lang.equals("ja", ignoreCase = true)) prevWord.trim() else SnapDictBinary.foldKey(prevWord.trim())
        val next = nextWord.trim()
        if (prev.isEmpty() || next.length !in 1..32 || next.any { it.isWhitespace() || it.isDigit() }) return
        val store = bigramsFor(lang)
        synchronized(store) {
            val followers = store.getOrPut(prev) { LinkedHashMap() }
            followers[next] = ((followers[next] ?: 0) + 1).coerceAtMost(10_000)
            if (followers.size > MAX_FOLLOWERS_PER_WORD) {
                val least = followers.entries.minByOrNull { it.value }?.key
                if (least != null) followers.remove(least)
            }
            if (store.size > MAX_BIGRAM_KEYS) {
                val firstKey = store.keys.firstOrNull()
                if (firstKey != null) store.remove(firstKey)
            }
            saveBigramsLocked(lang, store)
        }
    }

    fun nextWords(lang: String, prevKey: String, limit: Int = 3): List<String> {
        if (prevKey.isEmpty()) return emptyList()
        val store = bigramsFor(lang)
        synchronized(store) {
            val followers = store[prevKey] ?: return emptyList()
            return followers.entries
                .sortedByDescending { it.value }
                .take(limit)
                .map { it.key }
        }
    }

    fun forget(lang: String, word: String): Boolean {
        val map = mapFor(lang)
        synchronized(map) {
            val removed = map.remove(word) != null || map.remove(word.lowercase()) != null
            if (removed) saveLocked(lang, map)
            return removed
        }
    }

    fun clear(lang: String) {
        val map = mapFor(lang)
        synchronized(map) {
            map.clear()
            runCatching { fileFor(lang).delete() }
        }
        val bigrams = bigramsFor(lang)
        synchronized(bigrams) {
            bigrams.clear()
            runCatching { bigramFileFor(lang).delete() }
        }
    }

    fun matching(lang: String, foldedPrefix: String): List<Pair<String, Int>> {
        if (foldedPrefix.isEmpty()) return emptyList()
        val map = mapFor(lang)
        synchronized(map) {
            return map.entries
                .filter { SnapDictBinary.foldKey(it.key).startsWith(foldedPrefix) }
                .sortedByDescending { it.value }
                .take(8)
                .map { it.key to it.value }
        }
    }

    fun allCount(lang: String): Int {
        val map = mapFor(lang)
        synchronized(map) { return map.size }
    }

    private fun saveLocked(lang: String, map: Map<String, Int>) {
        val text = buildString {
            map.forEach { (w, c) -> append(w).append('\t').append(c).append('\n') }
        }
        val target = fileFor(lang)
        writer.execute {
            runCatching {
                dir.mkdirs()
                val tmp = File(dir, "${target.name}.tmp")
                tmp.writeText(text)
                if (!tmp.renameTo(target)) {
                    target.writeText(text)
                    tmp.delete()
                }
            }
        }
    }

    private fun saveBigramsLocked(lang: String, store: Map<String, Map<String, Int>>) {
        val text = buildString {
            store.forEach { (prev, followers) ->
                followers.forEach { (next, count) ->
                    append(prev).append('\t').append(next).append('\t').append(count).append('\n')
                }
            }
        }
        val target = bigramFileFor(lang)
        writer.execute {
            runCatching {
                dir.mkdirs()
                val tmp = File(dir, "${target.name}.tmp")
                tmp.writeText(text)
                if (!tmp.renameTo(target)) {
                    target.writeText(text)
                    tmp.delete()
                }
            }
        }
    }

    companion object {
        private const val MAX_WORDS = 1_500
        private const val MAX_BIGRAM_KEYS = 800
        private const val MAX_FOLLOWERS_PER_WORD = 8
    }
}
