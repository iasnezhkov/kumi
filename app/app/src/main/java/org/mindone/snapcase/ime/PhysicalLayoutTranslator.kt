// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import android.view.KeyEvent

object PhysicalLayoutTranslator {
    private val ruBaseLetters: Map<Int, Char> = mapOf(
        KeyEvent.KEYCODE_Q to 'й', KeyEvent.KEYCODE_W to 'ц', KeyEvent.KEYCODE_E to 'у', KeyEvent.KEYCODE_R to 'к',
        KeyEvent.KEYCODE_T to 'е', KeyEvent.KEYCODE_Y to 'н', KeyEvent.KEYCODE_U to 'г', KeyEvent.KEYCODE_I to 'ш',
        KeyEvent.KEYCODE_O to 'щ', KeyEvent.KEYCODE_P to 'з',
        KeyEvent.KEYCODE_A to 'ф', KeyEvent.KEYCODE_S to 'ы', KeyEvent.KEYCODE_D to 'в', KeyEvent.KEYCODE_F to 'а',
        KeyEvent.KEYCODE_G to 'п', KeyEvent.KEYCODE_H to 'р', KeyEvent.KEYCODE_J to 'о', KeyEvent.KEYCODE_K to 'л',
        KeyEvent.KEYCODE_L to 'д',
        KeyEvent.KEYCODE_Z to 'я', KeyEvent.KEYCODE_X to 'ч', KeyEvent.KEYCODE_C to 'с', KeyEvent.KEYCODE_V to 'м',
        KeyEvent.KEYCODE_B to 'и', KeyEvent.KEYCODE_N to 'т', KeyEvent.KEYCODE_M to 'ь',
    )

    private val ruFullExtras: Map<Int, Char> = mapOf(
        KeyEvent.KEYCODE_LEFT_BRACKET to 'х',
        KeyEvent.KEYCODE_RIGHT_BRACKET to 'ъ',
        KeyEvent.KEYCODE_GRAVE to 'ё',
        KeyEvent.KEYCODE_SEMICOLON to 'ж',
        KeyEvent.KEYCODE_APOSTROPHE to 'э',
        KeyEvent.KEYCODE_COMMA to 'б',
        KeyEvent.KEYCODE_PERIOD to 'ю',
    )

    private val ruLower: Map<Int, Char> = ruBaseLetters + ruFullExtras

    private val ukBaseLetters: Map<Int, Char> = ruBaseLetters + mapOf(
        KeyEvent.KEYCODE_S to 'і',
    )

    private val ukFullExtras: Map<Int, Char> = ruFullExtras + mapOf(
        KeyEvent.KEYCODE_RIGHT_BRACKET to 'ї',
        KeyEvent.KEYCODE_APOSTROPHE to 'є',
        KeyEvent.KEYCODE_GRAVE to 'ґ',
    )

    private val ukLower: Map<Int, Char> = ukBaseLetters + ukFullExtras

    private val enLower: Map<Int, Char> = mapOf(
        KeyEvent.KEYCODE_Q to 'q', KeyEvent.KEYCODE_W to 'w', KeyEvent.KEYCODE_E to 'e', KeyEvent.KEYCODE_R to 'r',
        KeyEvent.KEYCODE_T to 't', KeyEvent.KEYCODE_Y to 'y', KeyEvent.KEYCODE_U to 'u', KeyEvent.KEYCODE_I to 'i',
        KeyEvent.KEYCODE_O to 'o', KeyEvent.KEYCODE_P to 'p',
        KeyEvent.KEYCODE_A to 'a', KeyEvent.KEYCODE_S to 's', KeyEvent.KEYCODE_D to 'd', KeyEvent.KEYCODE_F to 'f',
        KeyEvent.KEYCODE_G to 'g', KeyEvent.KEYCODE_H to 'h', KeyEvent.KEYCODE_J to 'j', KeyEvent.KEYCODE_K to 'k',
        KeyEvent.KEYCODE_L to 'l',
        KeyEvent.KEYCODE_Z to 'z', KeyEvent.KEYCODE_X to 'x', KeyEvent.KEYCODE_C to 'c', KeyEvent.KEYCODE_V to 'v',
        KeyEvent.KEYCODE_B to 'b', KeyEvent.KEYCODE_N to 'n', KeyEvent.KEYCODE_M to 'm',
    )

    fun translate(
        keyCode: Int,
        shifted: Boolean,
        rawUnicode: Int,
        lang: StripLang,
        compactCaseKeyboard: Boolean = false,
        capsLock: Boolean = false,
        literalPunctuation: Boolean = false,
    ): Char? {
        val letterUpper = shifted xor capsLock
        if (lang == StripLang.JA) {
            if (!literalPunctuation) {
                if (keyCode == KeyEvent.KEYCODE_PERIOD) return if (shifted) '、' else '。'
                if (keyCode == KeyEvent.KEYCODE_COMMA) return '、'
            } else if (keyCode == KeyEvent.KEYCODE_PERIOD) {
                return if (shifted) ',' else '.'
            }
            if (keyCode == KeyEvent.KEYCODE_MINUS && !shifted) return '-'
            val letter = enLower[keyCode]
            if (letter != null) return if (letterUpper) letter.uppercaseChar() else letter
            return if (rawUnicode != 0) rawUnicode.toChar() else null
        }
        if (compactCaseKeyboard && keyCode == KeyEvent.KEYCODE_PERIOD) {
            return if (shifted) ',' else '.'
        }
        if (!compactCaseKeyboard && (lang == StripLang.RU || lang == StripLang.UK) && keyCode == KeyEvent.KEYCODE_SLASH) {
            return if (shifted) ',' else '.'
        }
        if (lang.isLatin && keyCode == KeyEvent.KEYCODE_PERIOD) {
            return if (rawUnicode != 0) rawUnicode.toChar() else if (shifted) '>' else '.'
        }
        val mapped = when (lang) {
            StripLang.RU -> if (compactCaseKeyboard) ruBaseLetters[keyCode] else ruLower[keyCode]
            StripLang.UK -> if (compactCaseKeyboard) ukBaseLetters[keyCode] else ukLower[keyCode]
            StripLang.JA -> enLower[keyCode]
            else -> enLower[keyCode]
        }
        if (mapped != null) {
            return if (letterUpper) mapped.uppercaseChar() else mapped
        }
        return if (rawUnicode != 0) rawUnicode.toChar() else null
    }
}
