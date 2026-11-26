// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime.dict

import kotlin.math.hypot
import kotlin.math.min

object SnapKeyboardGeometry {
    private val latinCoords: Map<Char, Pair<Float, Float>> = buildMap {
        "qwertyuiop".forEachIndexed { i, c -> put(c, i.toFloat() to 0f) }
        "asdfghjkl".forEachIndexed { i, c -> put(c, (i + 0.3f) to 1f) }
        "zxcvbnm".forEachIndexed { i, c -> put(c, (i + 0.8f) to 2f) }
    }

    private val cyrillicCoords: Map<Char, Pair<Float, Float>> = buildMap {
        "йцукенгшщзх".forEachIndexed { i, c -> put(c, i.toFloat() to 0f) }
        "фывапролджэ".forEachIndexed { i, c -> put(c, (i + 0.3f) to 1f) }
        "ячсмитьбю".forEachIndexed { i, c -> put(c, (i + 0.8f) to 2f) }
    }

    private val softSubstitutions: Set<Pair<Char, Char>> = setOf(
        'е' to 'э', 'э' to 'е',
        'з' to 'х', 'х' to 'з',
        'щ' to 'х', 'х' to 'щ',
        'д' to 'ж', 'ж' to 'д',
        'л' to 'ж', 'ж' to 'л',
        'и' to 'б', 'б' to 'и',
        'т' to 'б', 'б' to 'т',
        'м' to 'ю', 'ю' to 'м',
        'у' to 'ю', 'ю' to 'у',
        'ш' to 'щ', 'щ' to 'ш',
        'и' to 'ы', 'ы' to 'и',
        'и' to 'й', 'й' to 'и',
    )

    fun adjacentChars(ch: Char): Set<Char> {
        val table = if (ch in '\u0400'..'\u04FF') cyrillicCoords else latinCoords
        val origin = table[ch] ?: return setOf(ch)
        val result = HashSet<Char>(8)
        result.add(ch)
        for ((other, pos) in table) {
            if (hypot(origin.first - pos.first, origin.second - pos.second) <= 1.35f) {
                result.add(other)
            }
        }
        for ((a, b) in softSubstitutions) {
            if (a == ch) result.add(b)
        }
        return result
    }

    fun substitutionCost(a: Char, b: Char): Int {
        if (a == b) return 0
        if ((a to b) in softSubstitutions) return 4
        val table = if (a in '\u0400'..'\u04FF' && b in '\u0400'..'\u04FF') cyrillicCoords else latinCoords
        val p1 = table[a]
        val p2 = table[b]
        if (p1 != null && p2 != null) {
            val d = hypot(p1.first - p2.first, p1.second - p2.second)
            if (d <= 1.15f) return 5
            if (d <= 1.55f) return 7
        }
        return 10
    }

    fun editCost(a: String, b: String, maxCost: Int = 18): Int {
        val m = a.length
        val n = b.length
        if (kotlin.math.abs(m - n) * 5 > maxCost) return maxCost + 1
        var prev2 = IntArray(n + 1)
        var prev = IntArray(n + 1) { it * 10 }
        var curr = IntArray(n + 1)
        for (i in 1..m) {
            curr[0] = i * 10
            var rowMin = curr[0]
            val ca = a[i - 1]
            for (j in 1..n) {
                val cb = b[j - 1]
                val sub = prev[j - 1] + substitutionCost(ca, cb)
                val delCost = if (i > 1 && ca == a[i - 2]) 5 else 10
                val insCost = if (j > 1 && cb == b[j - 2]) 5 else 10
                val del = prev[j] + delCost
                val ins = curr[j - 1] + insCost
                var best = min(sub, min(del, ins))
                if (i > 1 && j > 1 && ca == b[j - 2] && a[i - 2] == cb) {
                    best = min(best, prev2[j - 2] + 5)
                }
                curr[j] = best
                if (best < rowMin) rowMin = best
            }
            if (rowMin > maxCost) return maxCost + 1
            val tmp = prev2
            prev2 = prev
            prev = curr
            curr = tmp
        }
        return prev[n]
    }
}
