// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime.dict

import org.mindone.snapcase.ime.TextSnippet

data class SuggestionResult(
    val suggestions: List<String>,
    val autoCorrectCandidate: String?,
)

class SuggestionEngine(
    private val personal: PersonalDictionary? = null,
) {
    fun suggest(
        rawInput: String,
        lang: String,
        dict: SnapDictBinary?,
        snippets: List<TextSnippet> = emptyList(),
        maxSuggestions: Int = 3,
        nextWordBoost: Set<String> = emptySet(),
    ): SuggestionResult {
        if (rawInput.isEmpty()) return SuggestionResult(emptyList(), null)

        val snippet = snippets.firstOrNull { it.trigger.equals(rawInput, ignoreCase = true) }
        val folded = SnapDictBinary.foldKey(rawInput)
        if (folded.isEmpty()) return SuggestionResult(emptyList(), null)

        val scored = LinkedHashMap<String, Double>(16)
        fun offer(word: String, score: Double) {
            val boost = if (nextWordBoost.isNotEmpty() && SnapDictBinary.foldKey(word) in nextWordBoost) 130.0 else 0.0
            val formatted = applyCase(rawInput, word)
            val existing = scored[formatted]
            val total = score + boost
            if (existing == null || total > existing) scored[formatted] = total
        }

        if (snippet != null) {
            scored[snippet.expansion] = 10_000.0
        }

        var exactInDict = false
        personal?.matching(lang, folded)?.forEach { (word, count) ->
            if (word.equals(rawInput, ignoreCase = true)) exactInDict = true
            val bonus = if (SnapDictBinary.foldKey(word) == folded) 500.0 else 320.0
            offer(word, bonus + count.coerceAtMost(200) * 1.5)
        }

        var foldedExactAccentMatch: String? = null
        if (dict != null) {
            val exacts = dict.exactMatches(folded)
            for (hit in exacts) {
                if (hit.word.equals(rawInput, ignoreCase = true)) {
                    exactInDict = true
                    offer(hit.word, 600.0 + hit.rank)
                } else {
                    if (foldedExactAccentMatch == null) foldedExactAccentMatch = applyCase(rawInput, hit.word)
                    offer(hit.word, 580.0 + hit.rank)
                }
            }

            val prefixes = dict.prefixMatches(folded, limit = 16)
            for (hit in prefixes) {
                if (hit.folded == folded) continue
                val lenPenalty = (hit.word.length - rawInput.length) * 14.0
                offer(hit.word, 280.0 + hit.rank - lenPenalty)
            }

            val isHangulWord = folded[0] in '\uAC00'..'\uD7A3' || folded[0] in '\u3131'..'\u318E'
            if (!isHangulWord && folded.length >= 2 && (!exactInDict || scored.size < maxSuggestions)) {
                val maxCost = if (folded.length <= 3) 10 else 15
                val firstNeighbors = SnapKeyboardGeometry.adjacentChars(folded[0])
                for (first in firstNeighbors) {
                    val indices = dict.bucketFor(first)
                    val firstPenalty = if (first == folded[0]) 0 else 4
                    for (idx in indices) {
                        val candFolded = dict.foldedAt(idx)
                        val lenDiff = kotlin.math.abs(candFolded.length - folded.length)
                        if (lenDiff > 1) continue
                        val cost = SnapKeyboardGeometry.editCost(folded, candFolded, maxCost - firstPenalty)
                        if (cost <= maxCost - firstPenalty && cost > 0) {
                            val rank = dict.rankAt(idx)
                            val score = 460.0 + rank * 1.2 - (cost + firstPenalty) * 28.0
                            offer(dict.wordAt(idx), score)
                        }
                    }
                }
            }
        }

        val ranked = scored.entries
            .sortedByDescending { it.value }
            .map { it.key }
            .take(maxSuggestions)

        val top = ranked.firstOrNull()
        val topScore = top?.let { scored[it] } ?: 0.0
        val autoCorrect = when {
            snippet != null -> snippet.expansion
            foldedExactAccentMatch != null && !exactInDict -> foldedExactAccentMatch
            folded.length >= 2 && !exactInDict && top != null && !top.equals(rawInput, ignoreCase = true) &&
                (topScore >= 340.0 || (folded.length >= 3 && topScore >= 250.0)) -> top
            else -> null
        }

        val finalChips = if (rawInput.length >= 2 && ranked.none { it.equals(rawInput, ignoreCase = true) } && ranked.size >= maxSuggestions) {
            ranked.take(maxSuggestions - 1) + rawInput
        } else {
            ranked
        }

        return SuggestionResult(finalChips.take(maxSuggestions), autoCorrect)
    }

    companion object {
        fun applyCase(reference: String, suggestion: String): String {
            if (reference.isEmpty() || suggestion.isEmpty()) return suggestion
            if (reference.length > 1 && reference.all { !it.isLetter() || it.isUpperCase() }) {
                return suggestion.uppercase()
            }
            if (reference[0].isUpperCase()) {
                return suggestion.replaceFirstChar { it.uppercaseChar() }
            }
            return suggestion
        }
    }
}
