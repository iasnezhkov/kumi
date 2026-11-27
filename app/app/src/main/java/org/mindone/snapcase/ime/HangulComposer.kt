// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

class HangulComposer {
    private val prefix = StringBuilder()
    private var cho: Int = -1
    private var jung: Int = -1
    private var jong: Int = 0

    fun isEmpty(): Boolean = prefix.isEmpty() && cho < 0

    fun clear() {
        prefix.setLength(0)
        cho = -1
        jung = -1
        jong = 0
    }

    fun setPrefixOnly(text: String) {
        clear()
        prefix.append(text)
    }

    fun text(): String {
        val active = currentChar()
        return if (active == null) prefix.toString() else prefix.toString() + active
    }

    fun feedLatin(rawChar: Char): Boolean {
        val jamo = latinToJamo(rawChar) ?: return false
        feedJamo(jamo)
        return true
    }

    fun feedJamo(jamo: Char) {
        val vowelIdx = JUNGSEONG.indexOf(jamo)
        if (vowelIdx >= 0) {
            handleVowel(vowelIdx, jamo)
            return
        }
        val choIdx = CHOSEONG.indexOf(jamo)
        if (choIdx >= 0) {
            handleConsonant(choIdx, jamo)
        }
    }

    fun backspace(): Boolean {
        if (jong > 0) {
            jong = COMPOUND_JONG_SPLIT[jong]?.first ?: 0
            return true
        }
        if (jung >= 0) {
            jung = COMPOUND_JUNG_SPLIT[jung]?.first ?: -1
            return true
        }
        if (cho >= 0) {
            cho = -1
            return true
        }
        if (prefix.isNotEmpty()) {
            prefix.deleteCharAt(prefix.lastIndex)
            return true
        }
        return false
    }

    fun dropLastPendingOrChar() {
        backspace()
    }

    private fun currentChar(): Char? {
        if (cho < 0) return null
        if (jung < 0) return CHOSEONG[cho]
        return (0xAC00 + (cho * 21 + jung) * 28 + jong).toChar()
    }

    private fun flushActive() {
        currentChar()?.let { prefix.append(it) }
        cho = -1
        jung = -1
        jong = 0
    }

    private fun handleVowel(vowelIdx: Int, vowelChar: Char) {
        if (cho < 0) {
            prefix.append(vowelChar)
            return
        }
        if (jung < 0) {
            jung = vowelIdx
            return
        }
        if (jong == 0) {
            val combined = COMPOUND_JUNG[jung to vowelIdx]
            if (combined != null) {
                jung = combined
            } else {
                flushActive()
                prefix.append(vowelChar)
            }
            return
        }
        val split = COMPOUND_JONG_SPLIT[jong]
        if (split != null) {
            jong = split.first
            flushActive()
            cho = split.second
            jung = vowelIdx
            jong = 0
        } else {
            val movedCho = JONG_TO_CHO[jong]
            jong = 0
            flushActive()
            if (movedCho != null) {
                cho = movedCho
                jung = vowelIdx
                jong = 0
            } else {
                prefix.append(vowelChar)
            }
        }
    }

    private fun handleConsonant(choIdx: Int, consChar: Char) {
        if (cho < 0) {
            cho = choIdx
            return
        }
        if (jung < 0) {
            flushActive()
            cho = choIdx
            return
        }
        if (jong == 0) {
            val jongIdx = JONGSEONG.indexOf(consChar)
            if (jongIdx > 0) {
                jong = jongIdx
            } else {
                flushActive()
                cho = choIdx
            }
            return
        }
        val combined = COMPOUND_JONG[jong to choIdx]
        if (combined != null) {
            jong = combined
        } else {
            flushActive()
            cho = choIdx
        }
    }

    companion object {
        private const val CHOSEONG = "ㄱㄲㄴㄷㄸㄹㅁㅂㅃㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ"
        private const val JUNGSEONG = "ㅏㅐㅑㅒㅓㅔㅕㅖㅗㅘㅙㅚㅛㅜㅝㅞㅟㅠㅡㅢㅣ"
        private const val JONGSEONG = "\u0000ㄱㄲㄳㄴㄵㄶㄷㄹㄺㄻㄼㄽㄾㄿㅀㅁㅂㅄㅅㅆㅇㅈㅊㅋㅌㅍㅎ"

        private val LATIN_MAP: Map<Char, Char> = mapOf(
            'r' to 'ㄱ', 'R' to 'ㄲ',
            's' to 'ㄴ',
            'e' to 'ㄷ', 'E' to 'ㄸ',
            'f' to 'ㄹ',
            'a' to 'ㅁ',
            'q' to 'ㅂ', 'Q' to 'ㅃ',
            't' to 'ㅅ', 'T' to 'ㅆ',
            'd' to 'ㅇ',
            'w' to 'ㅈ', 'W' to 'ㅉ',
            'c' to 'ㅊ',
            'z' to 'ㅋ',
            'x' to 'ㅌ',
            'v' to 'ㅍ',
            'g' to 'ㅎ',
            'k' to 'ㅏ',
            'o' to 'ㅐ', 'O' to 'ㅒ',
            'i' to 'ㅑ',
            'j' to 'ㅓ',
            'p' to 'ㅔ', 'P' to 'ㅖ',
            'u' to 'ㅕ',
            'h' to 'ㅗ',
            'y' to 'ㅛ',
            'n' to 'ㅜ',
            'b' to 'ㅠ',
            'm' to 'ㅡ',
            'l' to 'ㅣ',
        )

        private val COMPOUND_JUNG: Map<Pair<Int, Int>, Int> = mapOf(
            (8 to 0) to 9,
            (8 to 1) to 10,
            (8 to 20) to 11,
            (13 to 4) to 14,
            (13 to 5) to 15,
            (13 to 20) to 16,
            (18 to 20) to 19,
        )

        private val COMPOUND_JUNG_SPLIT: Map<Int, Pair<Int, Int>> = COMPOUND_JUNG.entries.associate { (k, v) -> v to k }

        private val COMPOUND_JONG: Map<Pair<Int, Int>, Int> = mapOf(
            (1 to 9) to 3,
            (4 to 12) to 5,
            (4 to 18) to 6,
            (8 to 0) to 9,
            (8 to 6) to 10,
            (8 to 7) to 11,
            (8 to 9) to 12,
            (8 to 16) to 13,
            (8 to 17) to 14,
            (8 to 18) to 15,
            (17 to 9) to 18,
        )

        private val COMPOUND_JONG_SPLIT: Map<Int, Pair<Int, Int>> = COMPOUND_JONG.entries.associate { (k, v) -> v to k }

        private val JONG_TO_CHO: Map<Int, Int> = buildMap {
            for (j in 1 until JONGSEONG.length) {
                val idx = CHOSEONG.indexOf(JONGSEONG[j])
                if (idx >= 0) put(j, idx)
            }
        }

        fun latinToJamo(rawChar: Char): Char? = LATIN_MAP[rawChar] ?: LATIN_MAP[rawChar.lowercaseChar()]

        fun isHangul(ch: Char): Boolean = ch in '\uAC00'..'\uD7A3' || ch in '\u3131'..'\u318E'

        fun composeFromLatin(input: String): String {
            val composer = HangulComposer()
            for (ch in input) {
                if (!composer.feedLatin(ch)) {
                    composer.flushActive()
                    composer.prefix.append(ch)
                }
            }
            return composer.text()
        }
    }
}
