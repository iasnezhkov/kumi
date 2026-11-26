// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime.dict

import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.text.Normalizer
import kotlin.math.ln
import kotlin.math.roundToInt

data class DictWord(val word: String, val folded: String, val rank: Int)

class SnapDictBinary(
    val count: Int,
    private val offsets: IntArray,
    private val lengths: ByteArray,
    private val ranks: ByteArray,
    private val pool: ByteArray,
) {
    private val wordCache = arrayOfNulls<String>(count)
    private val foldedCache = arrayOfNulls<String>(count)
    private val buckets: Map<Char, IntArray> by lazy { buildBuckets() }

    fun wordAt(index: Int): String {
        wordCache[index]?.let { return it }
        val text = String(pool, offsets[index], lengths[index].toInt() and 0xFF, Charsets.UTF_8)
        wordCache[index] = text
        return text
    }

    fun foldedAt(index: Int): String {
        foldedCache[index]?.let { return it }
        val text = foldKey(wordAt(index))
        foldedCache[index] = text
        return text
    }

    fun rankAt(index: Int): Int = ranks[index].toInt() and 0xFF

    fun exactMatches(foldedQuery: String): List<DictWord> {
        if (count == 0 || foldedQuery.isEmpty()) return emptyList()
        val start = lowerBound(foldedQuery)
        val out = ArrayList<DictWord>(4)
        var i = start
        while (i < count) {
            val f = foldedAt(i)
            if (f != foldedQuery) break
            out.add(DictWord(wordAt(i), f, rankAt(i)))
            i++
        }
        return out
    }

    fun prefixMatches(foldedPrefix: String, limit: Int = 24): List<DictWord> {
        if (count == 0 || foldedPrefix.isEmpty()) return emptyList()
        val start = lowerBound(foldedPrefix)
        val out = ArrayList<DictWord>(limit.coerceAtMost(32))
        var i = start
        val scanCap = (start + 160).coerceAtMost(count)
        while (i < scanCap) {
            val f = foldedAt(i)
            if (!f.startsWith(foldedPrefix)) break
            out.add(DictWord(wordAt(i), f, rankAt(i)))
            i++
        }
        out.sortWith(compareByDescending<DictWord> { it.folded == foldedPrefix }.thenByDescending { it.rank }.thenBy { it.word.length })
        return out.take(limit)
    }

    fun bucketFor(firstChar: Char): IntArray = buckets[firstChar] ?: EMPTY_INT_ARRAY

    private fun lowerBound(target: String): Int {
        var lo = 0
        var hi = count
        while (lo < hi) {
            val mid = (lo + hi) ushr 1
            if (foldedAt(mid) < target) lo = mid + 1 else hi = mid
        }
        return lo
    }

    private fun buildBuckets(): Map<Char, IntArray> {
        val grouped = HashMap<Char, ArrayList<Int>>(48)
        for (i in 0 until count) {
            val f = foldedAt(i)
            if (f.isNotEmpty()) {
                grouped.getOrPut(f[0]) { ArrayList(256) }.add(i)
            }
        }
        return grouped.mapValues { (_, list) -> list.toIntArray() }
    }

    companion object {
        private val MAGIC = "SNAPDCT1".toByteArray(Charsets.US_ASCII)
        private val EMPTY_INT_ARRAY = IntArray(0)

        fun foldKey(text: String): String {
            val sb = StringBuilder(text.length)
            val normalized = Normalizer.normalize(text.lowercase(), Normalizer.Form.NFD)
            for (ch in normalized) {
                val type = Character.getType(ch)
                if (type == Character.NON_SPACING_MARK.toInt()) continue
                val mapped = when (ch) {
                    'ё' -> 'е'
                    'ъ' -> 'ь'
                    'й' -> 'и'
                    'ґ' -> 'г'
                    'є' -> 'е'
                    'ї' -> 'и'
                    'і' -> 'и'
                    'ß' -> 's'
                    else -> ch
                }
                sb.append(mapped)
            }
            return sb.toString()
        }

        fun load(input: InputStream): SnapDictBinary {
            val data = input.readBytes()
            return fromBytes(data)
        }

        fun fromBytes(data: ByteArray): SnapDictBinary {
            require(data.size >= 12) { "dictionary too short" }
            for (i in MAGIC.indices) {
                require(data[i] == MAGIC[i]) { "invalid dictionary magic" }
            }
            val buf = ByteBuffer.wrap(data).order(ByteOrder.LITTLE_ENDIAN)
            val count = buf.getInt(8)
            val tableEnd = 12 + count * 6
            require(count >= 0 && tableEnd <= data.size) { "corrupt dictionary table" }
            val offsets = IntArray(count)
            val lengths = ByteArray(count)
            val ranks = ByteArray(count)
            val poolSize = data.size - tableEnd
            for (i in 0 until count) {
                val base = 12 + i * 6
                val off = buf.getInt(base)
                val len = data[base + 4]
                val rank = data[base + 5]
                require(off >= 0 && off + (len.toInt() and 0xFF) <= poolSize) { "corrupt dictionary entry" }
                offsets[i] = off
                lengths[i] = len
                ranks[i] = rank
            }
            val pool = data.copyOfRange(tableEnd, data.size)
            return SnapDictBinary(count, offsets, lengths, ranks, pool)
        }

        fun compileFromFrequencyText(text: String, maxWords: Int = 20_000, cyrillic: Boolean = false): ByteArray {
            val raw = ArrayList<Pair<String, Int>>(maxWords.coerceAtMost(20_000))
            val seen = HashSet<String>(maxWords.coerceAtMost(20_000))
            for (line in text.lineSequence()) {
                val trimmed = line.trim()
                if (trimmed.isEmpty()) continue
                val sep = trimmed.indexOfFirst { it == ' ' || it == '\t' }
                val word = (if (sep > 0) trimmed.substring(0, sep) else trimmed).lowercase()
                if (word.length !in 2..28) continue
                val valid = if (cyrillic) {
                    word.all { (it in '\u0400'..'\u04FF') || it == '-' || it == '\'' } && word.any { it in '\u0400'..'\u04FF' }
                } else {
                    word[0].isLetter() && word.all { it.isLetter() || it == '-' || it == '\'' }
                }
                if (!valid || !seen.add(word)) continue
                val freq = if (sep > 0) trimmed.substring(sep + 1).trim().toIntOrNull() ?: 1 else 1
                raw.add(word to freq.coerceAtLeast(1))
                if (raw.size >= maxWords) break
            }
            require(raw.isNotEmpty()) { "no valid words in frequency list" }
            val maxLog = ln(raw.first().second + 1.0)
            val minLog = ln(raw.last().second + 1.0)
            val span = (maxLog - minLog).coerceAtLeast(0.001)

            data class Row(val folded: String, val word: String, val rank: Int)
            val rows = raw.map { (w, f) ->
                val norm = (ln(f + 1.0) - minLog) / span
                val rank = (1 + (norm * 254.0).roundToInt()).coerceIn(1, 255)
                Row(foldKey(w), w, rank)
            }.sortedWith(compareBy<Row> { it.folded }.thenByDescending { it.rank }.thenBy { it.word })

            val pool = ByteArrayOutputStream(rows.size * 10)
            val table = ByteArrayOutputStream(rows.size * 6)
            val scratch = ByteBuffer.allocate(6).order(ByteOrder.LITTLE_ENDIAN)
            var kept = 0
            for (row in rows) {
                val bytes = row.word.toByteArray(Charsets.UTF_8)
                if (bytes.size > 255) continue
                val off = pool.size()
                pool.write(bytes)
                scratch.clear()
                scratch.putInt(off)
                scratch.put(bytes.size.toByte())
                scratch.put(row.rank.toByte())
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
    }
}
