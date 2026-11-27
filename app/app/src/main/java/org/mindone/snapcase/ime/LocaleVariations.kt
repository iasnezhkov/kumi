// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import android.view.KeyEvent
import java.util.Locale

enum class StripScript { LATIN, CYRILLIC, KANA, HANGUL, PINYIN }

enum class StripLang(val code: String, val label: String, val script: StripScript = StripScript.LATIN) {
    EN("en", "EN"),
    RU("ru", "RU", StripScript.CYRILLIC),
    UK("uk", "UK", StripScript.CYRILLIC),
    JA("ja", "JA", StripScript.KANA),
    KO("ko", "KO", StripScript.HANGUL),
    ZH_CN("zh-cn", "简", StripScript.PINYIN),
    ZH_TW("zh-tw", "繁", StripScript.PINYIN),
    ES("es", "ES"),
    DE("de", "DE"),
    FR("fr", "FR"),
    IT("it", "IT"),
    PT("pt", "PT"),
    PL("pl", "PL"),
    NL("nl", "NL"),
    TR("tr", "TR"),
    CS("cs", "CS"),
    SV("sv", "SV"),
    FI("fi", "FI"),
    RO("ro", "RO"),
    HU("hu", "HU"),
    ;

    val isLatin: Boolean get() = script == StripScript.LATIN
    val isCyrillic: Boolean get() = script == StripScript.CYRILLIC
    val isHangul: Boolean get() = script == StripScript.HANGUL
    val isPinyin: Boolean get() = script == StripScript.PINYIN
    val isCjkNoSpace: Boolean get() = script == StripScript.KANA || script == StripScript.PINYIN

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

    private val perLang: Map<StripLang, Map<Char, List<String>>> = mapOf(
        StripLang.ES to mapOf(
            'a' to listOf("á"), 'e' to listOf("é", "€"), 'i' to listOf("í"), 'o' to listOf("ó"),
            'u' to listOf("ú", "ü"), 'n' to listOf("ñ"),
            '.' to listOf(",", "?", "¿", "!", "¡", "…", "«", "»"),
            '?' to listOf("¿"), '!' to listOf("¡"),
        ),
        StripLang.DE to mapOf(
            'a' to listOf("ä"), 'o' to listOf("ö"), 'u' to listOf("ü"), 's' to listOf("ß"),
            'e' to listOf("€", "é"),
            '.' to listOf(",", "?", "!", "…", "—", "„", "“", "§"),
        ),
        StripLang.FR to mapOf(
            'e' to listOf("é", "è", "ê", "ë", "€"), 'a' to listOf("à", "â", "æ"), 'c' to listOf("ç"),
            'i' to listOf("î", "ï"), 'o' to listOf("ô", "œ"), 'u' to listOf("ù", "û", "ü"), 'y' to listOf("ÿ"),
            '.' to listOf(",", "?", "!", "…", "«", "»", ";", ":"),
        ),
        StripLang.IT to mapOf(
            'a' to listOf("à"), 'e' to listOf("è", "é", "€"), 'i' to listOf("ì"), 'o' to listOf("ò", "ó"),
            'u' to listOf("ù"),
            '.' to listOf(",", "?", "!", "…", "«", "»", ";", ":"),
        ),
        StripLang.PT to mapOf(
            'a' to listOf("ã", "á", "â", "à"), 'o' to listOf("õ", "ó", "ô"), 'c' to listOf("ç"),
            'e' to listOf("é", "ê", "€"), 'i' to listOf("í"), 'u' to listOf("ú", "ü"),
        ),
        StripLang.PL to mapOf(
            'a' to listOf("ą"), 'c' to listOf("ć"), 'e' to listOf("ę", "€"), 'l' to listOf("ł"),
            'n' to listOf("ń"), 'o' to listOf("ó"), 's' to listOf("ś"), 'z' to listOf("ż", "ź"),
        ),
        StripLang.NL to mapOf(
            'e' to listOf("é", "ë", "è", "€"), 'i' to listOf("ï", "í"), 'o' to listOf("ö", "ó"),
            'a' to listOf("ä", "á"), 'u' to listOf("ü", "ú"),
        ),
        StripLang.TR to mapOf(
            'c' to listOf("ç"), 'g' to listOf("ğ"), 'i' to listOf("ı", "İ"), 'o' to listOf("ö"),
            's' to listOf("ş"), 'u' to listOf("ü"),
        ),
        StripLang.CS to mapOf(
            'a' to listOf("á"), 'c' to listOf("č"), 'd' to listOf("ď"), 'e' to listOf("é", "ě"),
            'i' to listOf("í"), 'n' to listOf("ň"), 'o' to listOf("ó"), 'r' to listOf("ř"),
            's' to listOf("š"), 't' to listOf("ť"), 'u' to listOf("ú", "ů"), 'y' to listOf("ý"),
            'z' to listOf("ž"),
        ),
        StripLang.SV to mapOf(
            'a' to listOf("å", "ä"), 'o' to listOf("ö"), 'e' to listOf("é", "€"),
        ),
        StripLang.FI to mapOf(
            'a' to listOf("ä", "å"), 'o' to listOf("ö"), 's' to listOf("š"), 'z' to listOf("ž"),
        ),
        StripLang.RO to mapOf(
            'a' to listOf("ă", "â"), 'i' to listOf("î"), 's' to listOf("ș", "ş"), 't' to listOf("ț", "ţ"),
        ),
        StripLang.HU to mapOf(
            'a' to listOf("á"), 'e' to listOf("é"), 'i' to listOf("í"), 'o' to listOf("ó", "ö", "ő"),
            'u' to listOf("ú", "ü", "ű"),
        ),
    )

    private val extraRows: Map<StripLang, List<String>> = mapOf(
        StripLang.ES to listOf("ñ", "á", "é", "í", "ó", "ú", "ü", "¡", "¿", "«", "»", "€"),
        StripLang.DE to listOf("ä", "ö", "ü", "ß", "„", "“", "€", "§", "—", "…"),
        StripLang.FR to listOf("é", "è", "ê", "ë", "à", "â", "ç", "ù", "û", "ô", "œ", "«", "»"),
        StripLang.IT to listOf("à", "è", "é", "ì", "ò", "ó", "ù", "€", "«", "»"),
        StripLang.PT to listOf("ã", "õ", "ç", "á", "é", "í", "ó", "ú", "â", "ê", "ô", "à"),
        StripLang.PL to listOf("ą", "ć", "ę", "ł", "ń", "ó", "ś", "ź", "ż"),
        StripLang.NL to listOf("é", "è", "ë", "ï", "ó", "ö", "ü", "€", "—", "…"),
        StripLang.TR to listOf("ç", "ğ", "ı", "İ", "ö", "ş", "ü", "₺", "€"),
        StripLang.CS to listOf("á", "č", "ď", "é", "ě", "í", "ň", "ó", "ř", "š", "ť", "ú", "ů", "ý", "ž"),
        StripLang.SV to listOf("å", "ä", "ö", "é", "€", "—", "…"),
        StripLang.FI to listOf("ä", "ö", "å", "š", "ž", "€", "—", "…"),
        StripLang.RO to listOf("ă", "â", "î", "ș", "ț", "„", "”", "€", "—", "…"),
        StripLang.HU to listOf("á", "é", "í", "ó", "ö", "ő", "ú", "ü", "ű", "„", "”", "€"),
    )

    val ruExtraRow: List<String> = listOf("ё", "ъ", "э", "х", "ж", "б", "ю", "№", "«", "»", "—", "…")
    val ukExtraRow: List<String> = listOf("ґ", "є", "і", "ї", "х", "ж", "б", "ю", "'", "«", "»", "—")
    val jaExtraRow: List<String> = listOf("。", "、", "ー", "「", "」", "・", "？", "！", "〜", "（", "）", "…")
    val koExtraRow: List<String> = listOf(",", ".", "?", "!", "~", "₩", "ㅋ", "ㅎ", "ㅠ", "—", "…", "·")
    val zhCnExtraRow: List<String> = listOf("，", "。", "？", "！", "、", "：", "；", "“", "”", "（", "）", "……")
    val zhTwExtraRow: List<String> = listOf("，", "。", "？", "！", "、", "「", "」", "：", "；", "（", "）", "……")
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
            StripLang.EN, StripLang.JA, StripLang.KO, StripLang.ZH_CN, StripLang.ZH_TW -> latin[lower]
            else -> {
                val specific = perLang[lang]?.get(lower).orEmpty()
                val generic = latin[lower].orEmpty()
                (specific + generic).distinct().ifEmpty { null }
            }
        } ?: return emptyList()
        return if (upper) base.map { shiftExtra(it, true) } else base
    }

    fun extrasFor(lang: StripLang): List<String> = when (lang) {
        StripLang.RU -> ruExtraRow
        StripLang.UK -> ukExtraRow
        StripLang.JA -> jaExtraRow
        StripLang.KO -> koExtraRow
        StripLang.ZH_CN -> zhCnExtraRow
        StripLang.ZH_TW -> zhTwExtraRow
        StripLang.EN -> commonSymbols.take(12)
        else -> extraRows[lang] ?: commonSymbols.take(12)
    }

    fun shiftExtra(symbol: String, upper: Boolean): String {
        if (!upper || symbol.length != 1 || !symbol[0].isLetter() || symbol[0] == 'ß') return symbol
        return symbol.uppercase(Locale.ROOT)
    }
}
