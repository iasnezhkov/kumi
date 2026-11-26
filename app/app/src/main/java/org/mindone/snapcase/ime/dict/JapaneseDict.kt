// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime.dict

import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder

data class JaEntry(val reading: String, val surface: String, val rank: Int)

class JapaneseDict(
    val count: Int,
    private val readingOffsets: IntArray,
    private val readingLens: ByteArray,
    private val surfaceOffsets: IntArray,
    private val surfaceLens: ByteArray,
    private val ranks: ByteArray,
    private val pool: ByteArray,
) {
    private val readingCache = arrayOfNulls<String>(count)
    private val surfaceCache = arrayOfNulls<String>(count)

    fun readingAt(index: Int): String {
        readingCache[index]?.let { return it }
        val s = String(pool, readingOffsets[index], readingLens[index].toInt() and 0xFF, Charsets.UTF_8)
        readingCache[index] = s
        return s
    }

    fun surfaceAt(index: Int): String {
        surfaceCache[index]?.let { return it }
        val s = String(pool, surfaceOffsets[index], surfaceLens[index].toInt() and 0xFF, Charsets.UTF_8)
        surfaceCache[index] = s
        return s
    }

    fun rankAt(index: Int): Int = ranks[index].toInt() and 0xFF

    fun lookup(hiragana: String, limit: Int = 6): List<String> {
        if (count == 0 || hiragana.isEmpty()) return emptyList()
        val start = lowerBound(hiragana)
        val exact = ArrayList<Pair<String, Int>>(8)
        val prefix = ArrayList<Pair<String, Int>>(8)
        var i = start
        val cap = (start + 120).coerceAtMost(count)
        while (i < cap) {
            val r = readingAt(i)
            if (!r.startsWith(hiragana)) break
            val surface = surfaceAt(i)
            val rank = rankAt(i)
            if (r == hiragana) {
                exact.add(surface to rank)
            } else if (exact.size < limit) {
                prefix.add(surface to (rank - (r.length - hiragana.length) * 18))
            }
            i++
        }
        exact.sortByDescending { it.second }
        prefix.sortByDescending { it.second }
        val out = LinkedHashSet<String>(limit)
        exact.forEach { if (out.size < limit) out.add(it.first) }
        prefix.forEach { if (out.size < limit) out.add(it.first) }
        return out.toList()
    }

    private fun lowerBound(target: String): Int {
        var lo = 0
        var hi = count
        while (lo < hi) {
            val mid = (lo + hi) ushr 1
            if (readingAt(mid) < target) lo = mid + 1 else hi = mid
        }
        return lo
    }

    companion object {
        private val MAGIC = "SNAPJA01".toByteArray(Charsets.US_ASCII)

        fun load(input: InputStream): JapaneseDict = fromBytes(input.readBytes())

        fun fromBytes(data: ByteArray): JapaneseDict {
            require(data.size >= 12) { "ja dictionary too short" }
            for (i in MAGIC.indices) require(data[i] == MAGIC[i]) { "invalid ja dictionary magic" }
            val buf = ByteBuffer.wrap(data).order(ByteOrder.LITTLE_ENDIAN)
            val count = buf.getInt(8)
            val tableEnd = 12 + count * 11
            require(count >= 0 && tableEnd <= data.size) { "corrupt ja dictionary" }
            val rOff = IntArray(count)
            val rLen = ByteArray(count)
            val sOff = IntArray(count)
            val sLen = ByteArray(count)
            val ranks = ByteArray(count)
            val poolSize = data.size - tableEnd
            for (i in 0 until count) {
                val base = 12 + i * 11
                val ro = buf.getInt(base)
                val rl = data[base + 4]
                val so = buf.getInt(base + 5)
                val sl = data[base + 9]
                val rk = data[base + 10]
                require(ro >= 0 && ro + (rl.toInt() and 0xFF) <= poolSize) { "corrupt ja reading" }
                require(so >= 0 && so + (sl.toInt() and 0xFF) <= poolSize) { "corrupt ja surface" }
                rOff[i] = ro
                rLen[i] = rl
                sOff[i] = so
                sLen[i] = sl
                ranks[i] = rk
            }
            val pool = data.copyOfRange(tableEnd, data.size)
            return JapaneseDict(count, rOff, rLen, sOff, sLen, ranks, pool)
        }

        fun compileFromMozcTsv(text: String, maxEntries: Int = 25_000): ByteArray {
            val raw = ArrayList<JaEntry>(maxEntries)
            val seen = HashSet<String>(maxEntries)
            for (line in text.lineSequence()) {
                val parts = line.split('\t')
                if (parts.size < 5) continue
                val reading = parts[0].trim()
                val cost = parts[3].trim().toIntOrNull() ?: continue
                val surface = parts[4].trim()
                if (reading.isEmpty() || surface.isEmpty() || reading.length > 20 || surface.length > 20) continue
                if (!reading.all { it in '\u3041'..'\u3096' || it == 'ー' }) continue
                if (surface == reading && cost > 2500) continue
                if (!seen.add("$reading\u0000$surface")) continue
                val rank = (255 - (cost.coerceIn(0, 9000) * 254 / 9000)).coerceIn(1, 255)
                raw.add(JaEntry(reading, surface, rank))
            }
            val top = raw.sortedByDescending { it.rank }.take(maxEntries)
                .sortedWith(compareBy<JaEntry> { it.reading }.thenByDescending { it.rank }.thenBy { it.surface })
            val pool = ByteArrayOutputStream(top.size * 18)
            val table = ByteArrayOutputStream(top.size * 11)
            val scratch = ByteBuffer.allocate(11).order(ByteOrder.LITTLE_ENDIAN)
            var kept = 0
            for (e in top) {
                val rb = e.reading.toByteArray(Charsets.UTF_8)
                val sb = e.surface.toByteArray(Charsets.UTF_8)
                if (rb.size > 255 || sb.size > 255) continue
                val ro = pool.size()
                pool.write(rb)
                val so = pool.size()
                pool.write(sb)
                scratch.clear()
                scratch.putInt(ro)
                scratch.put(rb.size.toByte())
                scratch.putInt(so)
                scratch.put(sb.size.toByte())
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
    }
}
