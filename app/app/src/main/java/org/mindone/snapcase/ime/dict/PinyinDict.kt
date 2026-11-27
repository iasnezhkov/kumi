// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime.dict

import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.ln
import kotlin.math.roundToInt

data class PinyinEntry(val key: String, val text: String, val rank: Int)

class PinyinDict(
    val count: Int,
    private val keyOffsets: IntArray,
    private val keyLens: ByteArray,
    private val textOffsets: IntArray,
    private val textLens: ByteArray,
    private val ranks: ByteArray,
    private val pool: ByteArray,
) {
    private val keyCache = arrayOfNulls<String>(count)
    private val textCache = arrayOfNulls<String>(count)

    fun keyAt(index: Int): String {
        keyCache[index]?.let { return it }
        val s = String(pool, keyOffsets[index], keyLens[index].toInt() and 0xFF, Charsets.UTF_8)
        keyCache[index] = s
        return s
    }

    fun textAt(index: Int): String {
        textCache[index]?.let { return it }
        val s = String(pool, textOffsets[index], textLens[index].toInt() and 0xFF, Charsets.UTF_8)
        textCache[index] = s
        return s
    }

    fun rankAt(index: Int): Int = ranks[index].toInt() and 0xFF

    fun lookupExact(key: String, limit: Int = 8): List<Pair<String, Int>> {
        if (count == 0 || key.isEmpty()) return emptyList()
        val start = lowerBound(key)
        val out = ArrayList<Pair<String, Int>>(limit)
        var i = start
        val cap = (start + 96).coerceAtMost(count)
        while (i < cap) {
            if (keyAt(i) != key) break
            out.add(textAt(i) to rankAt(i))
            i++
        }
        out.sortByDescending { it.second }
        return out.take(limit)
    }

    fun lookupPrefix(prefix: String, limit: Int = 8): List<Pair<String, Int>> {
        if (count == 0 || prefix.isEmpty()) return emptyList()
        val start = lowerBound(prefix)
        val exact = ArrayList<Pair<String, Int>>(limit)
        val pref = ArrayList<Pair<String, Int>>(limit)
        var i = start
        val cap = (start + 180).coerceAtMost(count)
        while (i < cap) {
            val k = keyAt(i)
            if (!k.startsWith(prefix)) break
            val t = textAt(i)
            val r = rankAt(i)
            if (k == prefix) {
                exact.add(t to r)
            } else {
                pref.add(t to (r - (k.length - prefix.length) * 10))
            }
            i++
        }
        exact.sortByDescending { it.second }
        pref.sortByDescending { it.second }
        val out = LinkedHashMap<String, Int>(limit)
        exact.forEach { if (out.size < limit) out.putIfAbsent(it.first, it.second) }
        pref.forEach { if (out.size < limit) out.putIfAbsent(it.first, it.second) }
        return out.entries.map { it.key to it.value }
    }

    private fun lowerBound(target: String): Int {
        var lo = 0
        var hi = count
        while (lo < hi) {
            val mid = (lo + hi) ushr 1
            if (keyAt(mid) < target) lo = mid + 1 else hi = mid
        }
        return lo
    }

    companion object {
        private val MAGIC = "SNAPPY01".toByteArray(Charsets.US_ASCII)

        fun load(input: InputStream): PinyinDict = fromBytes(input.readBytes())

        fun fromBytes(data: ByteArray): PinyinDict {
            require(data.size >= 12) { "pinyin dictionary too short" }
            for (i in MAGIC.indices) require(data[i] == MAGIC[i]) { "invalid pinyin dictionary magic" }
            val buf = ByteBuffer.wrap(data).order(ByteOrder.LITTLE_ENDIAN)
            val count = buf.getInt(8)
            val tableEnd = 12 + count * 11
            require(count >= 0 && tableEnd <= data.size) { "corrupt pinyin dictionary" }
            val kOff = IntArray(count)
            val kLen = ByteArray(count)
            val tOff = IntArray(count)
            val tLen = ByteArray(count)
            val ranks = ByteArray(count)
            val poolSize = data.size - tableEnd
            for (i in 0 until count) {
                val base = 12 + i * 11
                val ko = buf.getInt(base)
                val kl = data[base + 4]
                val to = buf.getInt(base + 5)
                val tl = data[base + 9]
                val rk = data[base + 10]
                require(ko >= 0 && ko + (kl.toInt() and 0xFF) <= poolSize) { "corrupt pinyin key" }
                require(to >= 0 && to + (tl.toInt() and 0xFF) <= poolSize) { "corrupt pinyin text" }
                kOff[i] = ko
                kLen[i] = kl
                tOff[i] = to
                tLen[i] = tl
                ranks[i] = rk
            }
            val pool = data.copyOfRange(tableEnd, data.size)
            return PinyinDict(count, kOff, kLen, tOff, tLen, ranks, pool)
        }

        fun compileFromRimeYaml(
            yamlText: String,
            essayText: String? = null,
            maxEntries: Int = 28_000,
        ): ByteArray {
            val essayWeights = HashMap<String, Double>()
            if (!essayText.isNullOrEmpty()) {
                for (line in essayText.lineSequence()) {
                    val tab = line.indexOf('\t')
                    if (tab <= 0) continue
                    val word = line.substring(0, tab).trim()
                    val w = line.substring(tab + 1).trim().toDoubleOrNull() ?: continue
                    if (word.isNotEmpty() && w > 0.0) {
                        val prev = essayWeights[word]
                        if (prev == null || w > prev) essayWeights[word] = w
                    }
                }
            }
            var inBody = !yamlText.contains("\n...")
            val best = HashMap<Pair<String, String>, Double>(maxEntries)
            for (line in yamlText.lineSequence()) {
                val trimmed = line.trim()
                if (!inBody) {
                    if (trimmed == "...") inBody = true
                    continue
                }
                if (trimmed.isEmpty() || trimmed.startsWith('#')) continue
                val parts = trimmed.split('\t')
                if (parts.size < 2) continue
                val hanzi = parts[0].trim()
                if (hanzi.isEmpty() || hanzi.length > 8) continue
                if (!hanzi.all(::isCjkIdeograph)) continue
                val syllables = parts[1].trim().lowercase().split(' ').filter { it.isNotEmpty() }
                if (syllables.isEmpty() || syllables.size > 8) continue
                val compact = syllables.joinToString("") { it.replace("ü", "v") }
                if (compact.length > 36 || !compact.all { it in 'a'..'z' }) continue
                val explicitWeight = parts.getOrNull(2)?.trim()?.let(::parseWeight)
                val weight = when {
                    explicitWeight != null && explicitWeight > 0.0 -> explicitWeight
                    essayWeights.containsKey(hanzi) -> essayWeights[hanzi]!!
                    hanzi.length == 1 -> 1.0
                    else -> 0.25
                }
                val pair = compact to hanzi
                val prev = best[pair]
                if (prev == null || weight > prev) best[pair] = weight
            }
            require(best.isNotEmpty()) { "no valid pinyin entries" }
            val sorted = best.entries.sortedWith(
                compareByDescending<Map.Entry<Pair<String, String>, Double>> { it.value }
                    .thenBy { it.key.second.length }
                    .thenBy { it.key.first }
                    .thenBy { it.key.second },
            )
            val perKey = HashMap<String, Int>(16_384)
            val chosen = ArrayList<Pair<Pair<String, String>, Double>>(maxEntries)
            for (entry in sorted) {
                val k = entry.key.first
                val cap = if (entry.key.second.length == 1) 10 else 6
                val cnt = perKey[k] ?: 0
                if (cnt >= cap) continue
                perKey[k] = cnt + 1
                chosen.add(entry.key to entry.value)
                if (chosen.size >= maxEntries) break
            }
            val maxLog = ln(chosen.first().second + 1.0)
            val minLog = ln(chosen.last().second + 1.0)
            val span = (maxLog - minLog).coerceAtLeast(0.001)
            val rows = chosen.map { (pair, w) ->
                val norm = (ln(w + 1.0) - minLog) / span
                val rank = (1 + (norm * 254.0).roundToInt()).coerceIn(1, 255)
                PinyinEntry(pair.first, pair.second, rank)
            }.sortedWith(compareBy<PinyinEntry> { it.key }.thenByDescending { it.rank }.thenBy { it.text })

            return pack(rows)
        }

        fun pack(entries: List<PinyinEntry>): ByteArray {
            val sorted = entries.sortedWith(compareBy<PinyinEntry> { it.key }.thenByDescending { it.rank }.thenBy { it.text })
            val pool = ByteArrayOutputStream(sorted.size * 14)
            val table = ByteArrayOutputStream(sorted.size * 11)
            val scratch = ByteBuffer.allocate(11).order(ByteOrder.LITTLE_ENDIAN)
            var kept = 0
            for (e in sorted) {
                val kb = e.key.toByteArray(Charsets.UTF_8)
                val tb = e.text.toByteArray(Charsets.UTF_8)
                if (kb.size > 255 || tb.size > 255) continue
                val ko = pool.size()
                pool.write(kb)
                val to = pool.size()
                pool.write(tb)
                scratch.clear()
                scratch.putInt(ko)
                scratch.put(kb.size.toByte())
                scratch.putInt(to)
                scratch.put(tb.size.toByte())
                scratch.put(e.rank.toByte())
                table.write(scratch.array())
                kept++
            }
            val header = ByteBuffer.allocate(12).order(ByteOrder.LITTLE_ENDIAN)
            header.put(MAGIC)
            header.putInt(kept)
            val out = ByteArrayOutputStream(12 + table.size() + pool.size())
            out.write(header.array())
            out.write(table.toByteArray())
            out.write(pool.toByteArray())
            return out.toByteArray()
        }

        private fun parseWeight(raw: String): Double? {
            if (raw.isEmpty()) return null
            if (raw.endsWith('%')) {
                return raw.dropLast(1).toDoubleOrNull()?.let { it * 10.0 }
            }
            return raw.toDoubleOrNull()
        }

        private fun isCjkIdeograph(ch: Char): Boolean {
            val code = ch.code
            return code in 0x4E00..0x9FFF || code in 0x3400..0x4DBF
        }
    }
}
