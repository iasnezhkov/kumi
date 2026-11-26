// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

object RomajiKanaConverter {
    private val table: Map<String, String> = mapOf(
        "a" to "あ", "i" to "い", "u" to "う", "e" to "え", "o" to "お",
        "ka" to "か", "ki" to "き", "ku" to "く", "ke" to "け", "ko" to "こ",
        "sa" to "さ", "si" to "し", "shi" to "し", "su" to "す", "se" to "せ", "so" to "そ",
        "ta" to "た", "ti" to "ち", "chi" to "ち", "tu" to "つ", "tsu" to "つ", "te" to "て", "to" to "と",
        "na" to "な", "ni" to "に", "nu" to "ぬ", "ne" to "ね", "no" to "の",
        "ha" to "は", "hi" to "ひ", "hu" to "ふ", "fu" to "ふ", "he" to "へ", "ho" to "ほ",
        "ma" to "ま", "mi" to "み", "mu" to "む", "me" to "め", "mo" to "も",
        "ya" to "や", "yu" to "ゆ", "yo" to "よ",
        "ra" to "ら", "ri" to "り", "ru" to "る", "re" to "れ", "ro" to "ろ",
        "wa" to "わ", "wi" to "うぃ", "we" to "うぇ", "wo" to "を",
        "ga" to "が", "gi" to "ぎ", "gu" to "ぐ", "ge" to "げ", "go" to "ご",
        "za" to "ざ", "zi" to "じ", "ji" to "じ", "zu" to "ず", "ze" to "ぜ", "zo" to "ぞ",
        "da" to "だ", "di" to "ぢ", "du" to "づ", "de" to "で", "do" to "ど",
        "ba" to "ば", "bi" to "び", "bu" to "ぶ", "be" to "べ", "bo" to "ぼ",
        "pa" to "ぱ", "pi" to "ぴ", "pu" to "ぷ", "pe" to "ぺ", "po" to "ぽ",
        "kya" to "きゃ", "kyu" to "きゅ", "kyo" to "きょ",
        "sha" to "しゃ", "shu" to "しゅ", "sho" to "しょ", "sya" to "しゃ", "syu" to "しゅ", "syo" to "しょ",
        "cha" to "ちゃ", "chu" to "ちゅ", "cho" to "ちょ", "tya" to "ちゃ", "tyu" to "ちゅ", "tyo" to "ちょ",
        "nya" to "にゃ", "nyu" to "にゅ", "nyo" to "にょ",
        "hya" to "ひゃ", "hyu" to "ひゅ", "hyo" to "ひょ",
        "mya" to "みゃ", "myu" to "みゅ", "myo" to "みょ",
        "rya" to "りゃ", "ryu" to "りゅ", "ryo" to "りょ",
        "gya" to "ぎゃ", "gyu" to "ぎゅ", "gyo" to "ぎょ",
        "ja" to "じゃ", "ju" to "じゅ", "jo" to "じょ", "jya" to "じゃ", "jyu" to "じゅ", "jyo" to "じょ",
        "zya" to "じゃ", "zyu" to "じゅ", "zyo" to "じょ",
        "bya" to "びゃ", "byu" to "びゅ", "byo" to "びょ",
        "pya" to "ぴゃ", "pyu" to "ぴゅ", "pyo" to "ぴょ",
        "fa" to "ふぁ", "fi" to "ふぃ", "fe" to "ふぇ", "fo" to "ふぉ",
        "va" to "ゔぁ", "vi" to "ゔぃ", "vu" to "ゔ", "ve" to "ゔぇ", "vo" to "ゔぉ",
        "la" to "ぁ", "li" to "ぃ", "lu" to "ぅ", "le" to "ぇ", "lo" to "ぉ",
        "xa" to "ぁ", "xi" to "ぃ", "xu" to "ぅ", "xe" to "ぇ", "xo" to "ぉ",
        "lya" to "ゃ", "lyu" to "ゅ", "lyo" to "ょ", "xya" to "ゃ", "xyu" to "ゅ", "xyo" to "ょ",
        "ltu" to "っ", "ltsu" to "っ", "xtu" to "っ", "xtsu" to "っ",
        "n'" to "ん", "xn" to "ん",
        "-" to "ー",
    )

    private val vowels = setOf('a', 'i', 'u', 'e', 'o', 'y', 'n', '\'')
    private val nNextVowels = setOf('a', 'i', 'u', 'e', 'o', 'y')

    fun toHiragana(romaji: String, flushTrailingN: Boolean = false): String {
        val input = romaji.lowercase()
        val out = StringBuilder(input.length)
        var i = 0
        while (i < input.length) {
            val c = input[i]
            if (c !in 'a'..'z' && c != '-' && c != '\'') {
                out.append(c)
                i++
                continue
            }
            if (i + 1 < input.length && c == input[i + 1] && c in 'a'..'z' && c !in vowels) {
                out.append('っ')
                i++
                continue
            }
            if (c == 'n') {
                if (i + 1 < input.length) {
                    val next = input[i + 1]
                    if (next == '\'') {
                        out.append('ん')
                        i += 2
                        continue
                    }
                    if (next == 'n') {
                        val after = input.getOrNull(i + 2)
                        out.append('ん')
                        i += if (after != null && after in nNextVowels) 1 else 2
                        continue
                    }
                    if (next !in vowels && next in 'a'..'z') {
                        out.append('ん')
                        i++
                        continue
                    }
                } else if (flushTrailingN) {
                    out.append('ん')
                    i++
                    continue
                }
            }
            var matched = false
            for (len in 4 downTo 1) {
                if (i + len <= input.length) {
                    val sub = input.substring(i, i + len)
                    val kana = table[sub]
                    if (kana != null) {
                        out.append(kana)
                        i += len
                        matched = true
                        break
                    }
                }
            }
            if (!matched) {
                out.append(c)
                i++
            }
        }
        return out.toString()
    }

    fun hiraganaToKatakana(text: String): String {
        val sb = StringBuilder(text.length)
        for (ch in text) {
            if (ch in '\u3041'..'\u3096') {
                sb.append((ch.code + 0x60).toChar())
            } else {
                sb.append(ch)
            }
        }
        return sb.toString()
    }
}
