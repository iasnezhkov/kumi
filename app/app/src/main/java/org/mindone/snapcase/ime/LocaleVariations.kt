// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import android.view.KeyEvent

enum class StripLang(val code: String, val label: String) {
    EN("en", "EN"),
    RU("ru", "RU"),
    UK("uk", "UK"),
    JA("ja", "JA"),
    ;

    fun next(): StripLang = entries[(ordinal + 1) % entries.size]

    companion object {
        fun fromCode(code: String?): StripLang = entries.firstOrNull { it.code.equals(code, ignoreCase = true) } ?: EN
    }
}

object LocaleVariations {
    private val russian: Map<Char, List<String>> = mapOf(
        'е' to listOf("ё", "э"),
        'ь' to listOf("ъ"),
        'з' to listOf("х", "ъ"),
        'щ' to listOf("ъ", "х"),
        'д' to listOf("ж", "э"),
        'л' to listOf("ж"),
        'и' to listOf("б", "й"),
        'т' to listOf("б", "ю"),
        'м' to listOf("ю", "б"),
        'ш' to listOf("щ", "х"),
        'ч' to listOf("ц"),
        'у' to listOf("ю"),
        '.' to listOf(",", "?", "!", "…", "—", "«", "»"),
    )

    private val ukrainian: Map<Char, List<String>> = mapOf(
        'г' to listOf("ґ"),
        'е' to listOf("є", "ё"),
        'и' to listOf("і", "ї"),
        'ь' to listOf("ъ", "'"),
        'з' to listOf("х", "ї"),
        'д' to listOf("ж", "є"),
        'м' to listOf("ю", "б"),
        '.' to listOf(",", "?", "!", "…", "—", "«", "»"),
    )

    private val latin: Map<Char, List<String>> = mapOf(
        'a' to listOf("à", "á", "â", "ä", "å", "æ", "ã"),
        'e' to listOf("é", "è", "ê", "ë", "€"),
        'i' to listOf("í", "ì", "î", "ï"),
        'o' to listOf("ó", "ò", "ô", "ö", "ø", "œ", "õ"),
        'u' to listOf("ú", "ù", "û", "ü"),
        'c' to listOf("ç", "ć", "č"),
        'n' to listOf("ñ", "ń"),
        's' to listOf("ß", "ś", "š", "$"),
        'y' to listOf("ÿ", "ý"),
        'z' to listOf("ž", "ź", "ż"),
        '.' to listOf(",", "?", "!", "…", "—", ";", ":"),
        '-' to listOf("—", "–", "_"),
    )

    val ruExtraRow: List<String> = listOf("ё", "ъ", "э", "х", "ж", "б", "ю", "№", "«", "»", "—", "…")
    val ukExtraRow: List<String> = listOf("ґ", "є", "і", "ї", "х", "ж", "б", "ю", "'", "«", "»", "—")
    val jaExtraRow: List<String> = listOf("。", "、", "ー", "「", "」", "・", "？", "！", "〜", "（", "）", "…")
    val commonSymbols: List<String> = listOf(
        "!", "?", "@", "#", "$", "%", "^", "&", "*", "(", ")",
        "-", "_", "+", "=", "/", "\\", "|", "~", "`", "[", "]",
        "{", "}", "<", ">", ":", ";", "'", "\"", "€", "₽", "£", "¥",
        "°", "±", "×", "÷", "→", "←", "↑", "↓", "✓", "•", "—", "…",
    )

    private val mindOneKeycapSymbols: Map<Int, String> = mapOf(
        KeyEvent.KEYCODE_Q to "1",
        KeyEvent.KEYCODE_W to "2",
        KeyEvent.KEYCODE_E to "3",
        KeyEvent.KEYCODE_R to "4",
        KeyEvent.KEYCODE_T to "5",
        KeyEvent.KEYCODE_Y to "6",
        KeyEvent.KEYCODE_U to "7",
        KeyEvent.KEYCODE_I to "8",
        KeyEvent.KEYCODE_O to "9",
        KeyEvent.KEYCODE_P to "0",
        KeyEvent.KEYCODE_A to "@",
        KeyEvent.KEYCODE_S to "#",
        KeyEvent.KEYCODE_D to "$",
        KeyEvent.KEYCODE_F to "%",
        KeyEvent.KEYCODE_G to "&",
        KeyEvent.KEYCODE_H to "-",
        KeyEvent.KEYCODE_J to "+",
        KeyEvent.KEYCODE_K to "(",
        KeyEvent.KEYCODE_L to ")",
        KeyEvent.KEYCODE_Z to "!",
        KeyEvent.KEYCODE_X to "\"",
        KeyEvent.KEYCODE_C to "'",
        KeyEvent.KEYCODE_V to ":",
        KeyEvent.KEYCODE_B to ";",
        KeyEvent.KEYCODE_N to "/",
        KeyEvent.KEYCODE_M to "?",
    )

    fun mindOneKeycapSymbol(keyCode: Int): String? = mindOneKeycapSymbols[keyCode]

    fun forChar(ch: Char, lang: StripLang): List<String> {
        val upper = ch.isUpperCase()
        val lower = ch.lowercaseChar()
        val base = when (lang) {
            StripLang.RU -> russian[lower] ?: latin[lower]
            StripLang.UK -> ukrainian[lower] ?: russian[lower] ?: latin[lower]
            StripLang.EN, StripLang.JA -> latin[lower]
        } ?: return emptyList()
        return if (upper) base.map { it.uppercase() } else base
    }

    fun extrasFor(lang: StripLang): List<String> = when (lang) {
        StripLang.RU -> ruExtraRow
        StripLang.UK -> ukExtraRow
        StripLang.JA -> jaExtraRow
        StripLang.EN -> commonSymbols.take(12)
    }
}
