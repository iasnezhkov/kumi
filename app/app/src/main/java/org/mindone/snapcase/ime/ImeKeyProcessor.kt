// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import org.mindone.snapcase.ime.dict.JapaneseDict
import org.mindone.snapcase.ime.dict.NextWordPredictor
import org.mindone.snapcase.ime.dict.PersonalDictionary
import org.mindone.snapcase.ime.dict.PinyinDict
import org.mindone.snapcase.ime.dict.SnapDictBinary
import org.mindone.snapcase.ime.dict.SuggestionEngine

enum class LongPressBehavior { ALT_SYMBOL, VARIATIONS, UPPERCASE, OFF }

enum class HapticMode { OFF, EVENTS, ALL_KEYS }

data class ImeConfig(
    val stripEnabled: Boolean = true,
    val autoCorrect: Boolean = true,
    val autoCapitalize: Boolean = true,
    val doubleSpacePeriod: Boolean = true,
    val stickyShift: Boolean = false,
    val longPressBehavior: LongPressBehavior = LongPressBehavior.ALT_SYMBOL,
    val showQuickExtras: Boolean = true,
    val defaultLang: StripLang = StripLang.EN,
    val enabledLangs: List<StripLang> = listOf(StripLang.EN, StripLang.RU),
    val shiftSpaceSwitch: Boolean = true,
    val snippets: List<TextSnippet> = ClipboardStore.DEFAULT_SNIPPETS,
)

interface InputTarget {
    fun setComposing(text: String)
    fun commit(text: String)
    fun finishComposing()
    fun deleteSurrounding(before: Int, after: Int)
    fun textBeforeCursor(length: Int): String
}

data class UndoCorrection(
    val typed: String,
    val committed: String,
    val delimiter: String,
)

class ImeKeyProcessor(
    private val engine: SuggestionEngine,
    private val dictProvider: (StripLang) -> SnapDictBinary?,
    private val personal: PersonalDictionary? = null,
    private val nowMs: () -> Long = System::currentTimeMillis,
    private val jaDictProvider: () -> JapaneseDict? = { null },
    private val pinyinDictProvider: (StripLang) -> PinyinDict? = { null },
) {
    private val nextWordPredictor = NextWordPredictor(personal)
    private val composing = StringBuilder()
    private val jaRomaji = StringBuilder()
    private val pinyinRaw = StringBuilder()
    private val hangul = HangulComposer()
    private var lang: StripLang = StripLang.EN
    private var shiftState: StickyState = StickyState.OFF
    private var lastShiftTapMs: Long = 0L
    private var lastSpaceMs: Long = 0L
    private var lastTypedWasWordChar: Boolean = false
    private var pendingAutoCap: Boolean = false
    private var previousWord: String = ""
    private var autoCorrectCandidate: String? = null
    private var suggestions: List<String> = emptyList()
    private var selectedSuggestionIndex: Int = -1
    private var isNextWordPrediction: Boolean = false
    private var variations: List<String> = emptyList()
    private var lastSplitPrefix: String? = null
    private var lastSplitSymbol: String? = null
    private var undoRecord: UndoCorrection? = null
    private var suppressedAutoCorrectWord: String? = null
    private var isPasswordField: Boolean = false
    private var config: ImeConfig = ImeConfig()
    private var activeDrawer: StripDrawer = StripDrawer.NONE
    private var emojiQuery = StringBuilder()

    fun updateConfig(newConfig: ImeConfig) {
        val prevDefault = config.defaultLang
        config = newConfig
        if (!hasComposing()) {
            if (prevDefault != newConfig.defaultLang) {
                lang = newConfig.defaultLang
            } else if (newConfig.enabledLangs.isNotEmpty() && lang !in newConfig.enabledLangs) {
                lang = newConfig.enabledLangs.first()
            }
        }
    }

    fun startInput(passwordField: Boolean, textBefore: String) {
        composing.clear()
        jaRomaji.clear()
        pinyinRaw.clear()
        hangul.clear()
        suggestions = emptyList()
        selectedSuggestionIndex = -1
        isNextWordPrediction = false
        variations = emptyList()
        lastSplitPrefix = null
        lastSplitSymbol = null
        autoCorrectCandidate = null
        undoRecord = null
        suppressedAutoCorrectWord = null
        lastSpaceMs = 0L
        lastTypedWasWordChar = false
        isPasswordField = passwordField
        pendingAutoCap = !passwordField && config.autoCapitalize && shouldCapitalizeAfter(textBefore)
        previousWord = if (passwordField || pendingAutoCap) "" else extractPreviousWord(textBefore)
        if (shiftState == StickyState.ONE_SHOT) shiftState = StickyState.OFF
        if (!passwordField && previousWord.isNotEmpty() && textBefore.endsWith(" ")) {
            refreshNextWordPredictions()
        }
    }

    fun finishInput() {
        composing.clear()
        jaRomaji.clear()
        pinyinRaw.clear()
        hangul.clear()
        suggestions = emptyList()
        selectedSuggestionIndex = -1
        isNextWordPrediction = false
        previousWord = ""
        variations = emptyList()
        lastSplitPrefix = null
        lastSplitSymbol = null
        autoCorrectCandidate = null
        undoRecord = null
        activeDrawer = StripDrawer.NONE
        emojiQuery.clear()
    }

    fun setDrawer(drawer: StripDrawer) {
        activeDrawer = if (activeDrawer == drawer) StripDrawer.NONE else drawer
        if (activeDrawer != StripDrawer.EMOJI) emojiQuery.clear()
    }

    fun closeDrawer() {
        activeDrawer = StripDrawer.NONE
        emojiQuery.clear()
    }

    fun currentLanguage(): StripLang = lang

    fun hasComposing(): Boolean = composing.isNotEmpty() || jaRomaji.isNotEmpty() || pinyinRaw.isNotEmpty() || !hangul.isEmpty()

    fun hasSuggestions(): Boolean = suggestions.isNotEmpty()

    fun hasSelectedSuggestion(): Boolean = selectedSuggestionIndex in suggestions.indices

    fun moveSuggestionSelection(delta: Int): Boolean {
        if (suggestions.isEmpty() || delta == 0) return false
        val size = suggestions.size
        if (selectedSuggestionIndex !in suggestions.indices) {
            selectedSuggestionIndex = if (delta > 0) {
                val autoFirst = !isNextWordPrediction && (config.autoCorrect || lang.isCjkNoSpace) &&
                    autoCorrectCandidate != null && suggestions.firstOrNull() == autoCorrectCandidate
                if (autoFirst && size > 1) 1 else 0
            } else {
                size - 1
            }
            return true
        }
        val next = selectedSuggestionIndex + delta
        if (delta < 0 && next < 0) {
            selectedSuggestionIndex = -1
            return true
        }
        selectedSuggestionIndex = ((next % size) + size) % size
        return true
    }

    fun selectFirstSuggestion(): Boolean {
        if (suggestions.isEmpty()) return false
        selectedSuggestionIndex = 0
        return true
    }

    fun clearSuggestionSelection(): Boolean {
        if (selectedSuggestionIndex < 0) return false
        selectedSuggestionIndex = -1
        return true
    }

    fun acceptSelectedSuggestion(target: InputTarget): Boolean {
        val idx = selectedSuggestionIndex
        if (idx !in suggestions.indices) return false
        pickSuggestion(suggestions[idx], target)
        return true
    }

    fun acceptSuggestionAt(index: Int, target: InputTarget): Boolean {
        if (index !in suggestions.indices) return false
        pickSuggestion(suggestions[index], target)
        return true
    }

    fun cycleLanguage(target: InputTarget? = null): StripLang {
        if (target != null && hasComposing()) {
            commitCurrentWord(target, delimiter = "", applyAutoCorrect = false)
        }
        val cycle = config.enabledLangs.ifEmpty { listOf(StripLang.EN, StripLang.RU) }
        val idx = cycle.indexOf(lang)
        lang = if (idx < 0) cycle.first() else cycle[(idx + 1) % cycle.size]
        variations = emptyList()
        lastSplitPrefix = null
        lastSplitSymbol = null
        undoRecord = null
        selectedSuggestionIndex = -1
        previousWord = ""
        refreshSuggestions()
        return lang
    }

    fun setLanguage(next: StripLang, target: InputTarget? = null) {
        if (target != null && hasComposing()) {
            commitCurrentWord(target, delimiter = "", applyAutoCorrect = false)
        }
        lang = next
        variations = emptyList()
        lastSplitPrefix = null
        lastSplitSymbol = null
        undoRecord = null
        selectedSuggestionIndex = -1
        previousWord = ""
        refreshSuggestions()
    }

    fun onShiftTap() {
        if (!config.stickyShift) return
        val now = nowMs()
        shiftState = when (shiftState) {
            StickyState.OFF -> StickyState.ONE_SHOT
            StickyState.ONE_SHOT -> if (now - lastShiftTapMs <= DOUBLE_TAP_MS) StickyState.LOCKED else StickyState.OFF
            StickyState.LOCKED -> StickyState.OFF
        }
        lastShiftTapMs = now
    }

    fun handleEmojiKey(ch: Char?, isBackspace: Boolean, isEnter: Boolean, isEscape: Boolean, firstEmoji: String?, target: InputTarget): Boolean {
        if (activeDrawer != StripDrawer.EMOJI) return false
        if (isEscape) {
            closeDrawer()
            return true
        }
        if (isEnter) {
            if (!firstEmoji.isNullOrEmpty()) {
                commitEmoji(firstEmoji, target)
            }
            return true
        }
        if (isBackspace) {
            if (emojiQuery.isNotEmpty()) {
                emojiQuery.deleteCharAt(emojiQuery.lastIndex)
                return true
            }
            return false
        }
        if (ch != null && !ch.isISOControl()) {
            emojiQuery.append(ch)
            return true
        }
        return false
    }

    fun commitEmoji(glyph: String, target: InputTarget) {
        if (composing.isNotEmpty()) {
            commitCurrentWord(target, delimiter = "", applyAutoCorrect = false)
        }
        target.commit(glyph)
        undoRecord = null
        lastTypedWasWordChar = false
    }

    fun consumeEffectiveUpper(rawChar: Char, physicalUpper: Boolean): Char {
        val applyUpper = !physicalUpper && rawChar.isLetter() && shiftState != StickyState.OFF
        if (shiftState == StickyState.ONE_SHOT) shiftState = StickyState.OFF
        return if (applyUpper) rawChar.uppercaseChar() else rawChar
    }

    fun onCharTyped(
        rawChar: Char,
        physicalShifted: Boolean,
        target: InputTarget,
        autoDetectLang: Boolean = true,
        isRepeat: Boolean = false,
    ): Boolean {
        lastSplitPrefix = null
        lastSplitSymbol = null
        if (isPasswordField) {
            variations = emptyList()
            if (shiftState == StickyState.ONE_SHOT) shiftState = StickyState.OFF
            return false
        }
        if (lang == StripLang.JA) {
            if (shiftState == StickyState.ONE_SHOT) shiftState = StickyState.OFF
            return onJapaneseCharTyped(rawChar, target)
        }
        if (lang.isPinyin) {
            if (shiftState == StickyState.ONE_SHOT) shiftState = StickyState.OFF
            return onPinyinCharTyped(rawChar, target)
        }
        if (lang.isHangul) {
            val effectiveChar = if (!physicalShifted && rawChar.isLetter() && shiftState != StickyState.OFF) {
                rawChar.uppercaseChar()
            } else {
                rawChar
            }
            if (shiftState == StickyState.ONE_SHOT) shiftState = StickyState.OFF
            return onHangulCharTyped(effectiveChar, target, isRepeat)
        }
        if (autoDetectLang) detectLanguageFromChar(rawChar)

        val applyUpper = !physicalShifted && rawChar.isLetter() && (
            shiftState != StickyState.OFF || (pendingAutoCap && composing.isEmpty())
            )
        val ch = if (applyUpper) rawChar.uppercaseChar() else rawChar
        if (shiftState == StickyState.ONE_SHOT) shiftState = StickyState.OFF
        if (pendingAutoCap && rawChar.isLetter()) pendingAutoCap = false

        if (isWordChar(ch)) {
            undoRecord = null
            variations = emptyList()
            selectedSuggestionIndex = -1
            isNextWordPrediction = false
            composing.append(ch)
            target.setComposing(composing.toString())
            lastTypedWasWordChar = true
            refreshSuggestions()
            return true
        }

        variations = emptyList()

        if (ch == ' ') {
            if (selectedSuggestionIndex in suggestions.indices) {
                pickSuggestion(suggestions[selectedSuggestionIndex], target)
                return true
            }
            val now = nowMs()
            if (!isRepeat && composing.isEmpty() && config.doubleSpacePeriod && lastTypedWasWordChar &&
                now - lastSpaceMs in 1..DOUBLE_SPACE_MS && target.textBeforeCursor(2).endsWith(" ")
            ) {
                val before = target.textBeforeCursor(3)
                if (before.length >= 2 && before[before.length - 2].isLetterOrDigit()) {
                    target.deleteSurrounding(1, 0)
                    target.commit(". ")
                    pendingAutoCap = config.autoCapitalize
                    previousWord = ""
                    suggestions = emptyList()
                    isNextWordPrediction = false
                    selectedSuggestionIndex = -1
                    lastSpaceMs = 0L
                    lastTypedWasWordChar = false
                    undoRecord = null
                    return true
                }
            }
            if (composing.isNotEmpty()) {
                val hadWord = composing.isNotEmpty()
                commitCurrentWord(target, delimiter = " ", applyAutoCorrect = config.autoCorrect)
                lastSpaceMs = if (hadWord) now else 0L
                lastTypedWasWordChar = hadWord
                refreshNextWordPredictions()
                return true
            }
            lastSpaceMs = now
            return false
        }

        if (ch in SENTENCE_END || ch in WORD_DELIMITERS) {
            selectedSuggestionIndex = -1
            if (composing.isNotEmpty()) {
                commitCurrentWord(target, delimiter = ch.toString(), applyAutoCorrect = config.autoCorrect)
            } else {
                target.commit(ch.toString())
            }
            if (ch in SENTENCE_END) {
                if (config.autoCapitalize) pendingAutoCap = true
                previousWord = ""
            } else {
                pendingAutoCap = false
            }
            suggestions = emptyList()
            isNextWordPrediction = false
            lastTypedWasWordChar = false
            lastSpaceMs = 0L
            return true
        }

        if (composing.isNotEmpty()) {
            commitCurrentWord(target, delimiter = "", applyAutoCorrect = false)
        }
        return false
    }

    fun onLongPressRepeat(
        rawChar: Char,
        target: InputTarget,
        keyCode: Int = 0,
        compactCaseKeyboard: Boolean = false,
        numericField: Boolean = false,
    ): Boolean {
        if (config.longPressBehavior == LongPressBehavior.OFF) return false
        val keycapSym = if (compactCaseKeyboard) LocaleVariations.mindOneKeycapSymbol(keyCode) else null
        if (isPasswordField) {
            if (config.longPressBehavior == LongPressBehavior.ALT_SYMBOL && keycapSym != null) {
                if (!numericField) {
                    val prev = target.textBeforeCursor(1)
                    if (prev.isEmpty() || prev[0].equals(rawChar, ignoreCase = true) || prev[0] == '\u2022' || prev[0] == '*') {
                        target.deleteSurrounding(1, 0)
                    }
                }
                target.commit(keycapSym)
                return true
            }
            return false
        }
        val effectiveChar = if (composing.isNotEmpty() && composing.last().isUpperCase() && rawChar.isLetter()) {
            rawChar.uppercaseChar()
        } else {
            rawChar
        }
        return when (config.longPressBehavior) {
            LongPressBehavior.ALT_SYMBOL -> {
                val alts = LocaleVariations.forChar(effectiveChar, lang)
                if (keycapSym != null) {
                    lastSplitPrefix = null
                    lastSplitSymbol = null
                    if (lang == StripLang.JA && jaRomaji.isNotEmpty()) {
                        jaRomaji.deleteCharAt(jaRomaji.lastIndex)
                        if (jaRomaji.isNotEmpty()) {
                            val kana = RomajiKanaConverter.toHiragana(jaRomaji.toString(), flushTrailingN = false)
                            composing.clear()
                            composing.append(kana)
                            commitCurrentWord(target, delimiter = keycapSym, applyAutoCorrect = false)
                        } else {
                            composing.clear()
                            target.setComposing("")
                            target.commit(keycapSym)
                            suggestions = emptyList()
                            autoCorrectCandidate = null
                            selectedSuggestionIndex = -1
                            isNextWordPrediction = false
                        }
                    } else if (lang.isPinyin && pinyinRaw.isNotEmpty()) {
                        pinyinRaw.deleteCharAt(pinyinRaw.lastIndex)
                        if (pinyinRaw.isNotEmpty()) {
                            val formatted = PinyinEngine.formatComposing(pinyinRaw.toString())
                            composing.clear()
                            composing.append(formatted)
                            commitCurrentWord(target, delimiter = keycapSym, applyAutoCorrect = false)
                        } else {
                            composing.clear()
                            target.setComposing("")
                            target.commit(keycapSym)
                            suggestions = emptyList()
                            autoCorrectCandidate = null
                            selectedSuggestionIndex = -1
                            isNextWordPrediction = false
                        }
                    } else if (lang.isHangul && !hangul.isEmpty()) {
                        hangul.dropLastPendingOrChar()
                        val remaining = hangul.text()
                        if (remaining.isNotEmpty()) {
                            composing.clear()
                            composing.append(remaining)
                            commitCurrentWord(target, delimiter = keycapSym, applyAutoCorrect = false)
                        } else {
                            composing.clear()
                            target.setComposing("")
                            target.commit(keycapSym)
                            suggestions = emptyList()
                            autoCorrectCandidate = null
                            selectedSuggestionIndex = -1
                            isNextWordPrediction = false
                        }
                    } else if (composing.isNotEmpty()) {
                        composing.deleteCharAt(composing.lastIndex)
                        if (composing.isNotEmpty()) {
                            val prefix = composing.toString()
                            commitCurrentWord(target, delimiter = keycapSym, applyAutoCorrect = false)
                            lastSplitPrefix = prefix
                            lastSplitSymbol = keycapSym
                        } else {
                            target.setComposing("")
                            target.commit(keycapSym)
                            suggestions = emptyList()
                            autoCorrectCandidate = null
                            selectedSuggestionIndex = -1
                            isNextWordPrediction = false
                        }
                    } else {
                        if (!numericField) {
                            val prev = target.textBeforeCursor(1)
                            if (prev.length == 1 && prev[0].equals(rawChar, ignoreCase = true)) {
                                target.deleteSurrounding(1, 0)
                            }
                        }
                        target.commit(keycapSym)
                        suggestions = emptyList()
                        autoCorrectCandidate = null
                        selectedSuggestionIndex = -1
                        isNextWordPrediction = false
                    }
                    if (keycapSym.length == 1 && keycapSym[0] in SENTENCE_END) {
                        if (config.autoCapitalize) pendingAutoCap = true
                        previousWord = ""
                    } else {
                        pendingAutoCap = false
                    }
                    undoRecord = null
                    lastTypedWasWordChar = false
                    variations = alts
                    true
                } else if (alts.isNotEmpty()) {
                    variations = alts
                    true
                } else {
                    false
                }
            }
            LongPressBehavior.UPPERCASE -> {
                if (!rawChar.isLetter()) return false
                val upper = rawChar.uppercaseChar()
                if (lang == StripLang.JA && jaRomaji.isNotEmpty()) {
                    jaRomaji.deleteCharAt(jaRomaji.lastIndex)
                    if (jaRomaji.isNotEmpty()) {
                        val kana = RomajiKanaConverter.toHiragana(jaRomaji.toString(), flushTrailingN = false)
                        composing.clear()
                        composing.append(kana)
                        commitCurrentWord(target, delimiter = upper.toString(), applyAutoCorrect = false)
                    } else {
                        composing.clear()
                        target.setComposing("")
                        target.commit(upper.toString())
                        suggestions = emptyList()
                        autoCorrectCandidate = null
                        selectedSuggestionIndex = -1
                        isNextWordPrediction = false
                    }
                } else if (composing.isNotEmpty()) {
                    composing.setCharAt(composing.lastIndex, upper)
                    target.setComposing(composing.toString())
                    refreshSuggestions()
                } else {
                    target.deleteSurrounding(1, 0)
                    target.commit(upper.toString())
                }
                true
            }
            LongPressBehavior.VARIATIONS -> {
                val localeAlts = LocaleVariations.forChar(effectiveChar, lang)
                val combined = buildList {
                    if (keycapSym != null) add(keycapSym)
                    localeAlts.forEach { if (it != keycapSym) add(it) }
                }
                if (combined.isEmpty()) return false
                variations = combined
                true
            }
            LongPressBehavior.OFF -> false
        }
    }

    fun onBackspace(target: InputTarget): Boolean {
        variations = emptyList()
        lastSplitPrefix = null
        lastSplitSymbol = null
        selectedSuggestionIndex = -1
        if (lang == StripLang.JA && jaRomaji.isNotEmpty()) {
            jaRomaji.deleteCharAt(jaRomaji.lastIndex)
            if (jaRomaji.isEmpty()) {
                composing.clear()
                target.setComposing("")
                suggestions = emptyList()
                isNextWordPrediction = false
                autoCorrectCandidate = null
            } else {
                val kana = RomajiKanaConverter.toHiragana(jaRomaji.toString(), flushTrailingN = false)
                composing.clear()
                composing.append(kana)
                target.setComposing(kana)
                refreshSuggestions()
            }
            return true
        }
        if (lang.isPinyin && pinyinRaw.isNotEmpty()) {
            pinyinRaw.deleteCharAt(pinyinRaw.lastIndex)
            if (pinyinRaw.isEmpty()) {
                composing.clear()
                target.setComposing("")
                suggestions = emptyList()
                isNextWordPrediction = false
                autoCorrectCandidate = null
            } else {
                val formatted = PinyinEngine.formatComposing(pinyinRaw.toString())
                composing.clear()
                composing.append(formatted)
                target.setComposing(formatted)
                refreshSuggestions()
            }
            return true
        }
        if (lang.isHangul && !hangul.isEmpty()) {
            hangul.backspace()
            val text = hangul.text()
            composing.clear()
            composing.append(text)
            if (text.isEmpty()) {
                target.setComposing("")
                suggestions = emptyList()
                isNextWordPrediction = false
                autoCorrectCandidate = null
            } else {
                target.setComposing(text)
                refreshSuggestions()
            }
            return true
        }
        if (composing.isNotEmpty()) {
            composing.deleteCharAt(composing.lastIndex)
            if (composing.isEmpty()) {
                target.setComposing("")
                suggestions = emptyList()
                isNextWordPrediction = false
                autoCorrectCandidate = null
                pendingAutoCap = !isPasswordField && config.autoCapitalize && shouldCapitalizeAfter(target.textBeforeCursor(32))
            } else {
                target.setComposing(composing.toString())
                refreshSuggestions()
            }
            return true
        }
        val undo = undoRecord
        if (undo != null) {
            val expected = undo.committed + undo.delimiter
            val before = target.textBeforeCursor(expected.length)
            if (before == expected) {
                target.deleteSurrounding(expected.length, 0)
                composing.clear()
                jaRomaji.clear()
                pinyinRaw.clear()
                hangul.clear()
                when {
                    lang == StripLang.JA -> {
                        jaRomaji.append(undo.typed)
                        val kana = RomajiKanaConverter.toHiragana(undo.typed, flushTrailingN = false)
                        composing.append(kana)
                        target.setComposing(kana)
                    }
                    lang.isPinyin -> {
                        pinyinRaw.append(undo.typed)
                        val formatted = PinyinEngine.formatComposing(undo.typed)
                        composing.append(formatted)
                        target.setComposing(formatted)
                    }
                    lang.isHangul -> {
                        hangul.setPrefixOnly(undo.typed)
                        composing.append(undo.typed)
                        target.setComposing(undo.typed)
                    }
                    else -> {
                        composing.append(undo.typed)
                        target.setComposing(composing.toString())
                    }
                }
                suppressedAutoCorrectWord = undo.typed
                undoRecord = null
                refreshSuggestions()
                return true
            }
            undoRecord = null
        }
        if (isNextWordPrediction) {
            suggestions = emptyList()
            isNextWordPrediction = false
            previousWord = ""
        }
        if (!isPasswordField && config.autoCapitalize) {
            val before = target.textBeforeCursor(32)
            val afterDelete = if (before.isNotEmpty()) before.dropLast(1) else ""
            pendingAutoCap = shouldCapitalizeAfter(afterDelete)
        }
        return false
    }

    fun onEnter(target: InputTarget): Boolean {
        variations = emptyList()
        if (selectedSuggestionIndex in suggestions.indices) {
            pickSuggestion(suggestions[selectedSuggestionIndex], target)
            return true
        }
        if (lang == StripLang.JA && jaRomaji.isNotEmpty()) {
            val kana = RomajiKanaConverter.toHiragana(jaRomaji.toString(), flushTrailingN = true)
            jaRomaji.clear()
            composing.clear()
            target.commit(kana)
            recordCommittedWord(kana)
            suggestions = emptyList()
            isNextWordPrediction = false
            autoCorrectCandidate = null
            undoRecord = null
            return true
        }
        if (lang.isPinyin && pinyinRaw.isNotEmpty()) {
            val raw = pinyinRaw.toString()
            pinyinRaw.clear()
            composing.clear()
            target.commit(raw)
            suggestions = emptyList()
            isNextWordPrediction = false
            autoCorrectCandidate = null
            undoRecord = null
            return true
        }
        if (hasComposing()) {
            commitCurrentWord(target, delimiter = "", applyAutoCorrect = false)
        }
        if (config.autoCapitalize) pendingAutoCap = true
        previousWord = ""
        suggestions = emptyList()
        isNextWordPrediction = false
        selectedSuggestionIndex = -1
        undoRecord = null
        lastSpaceMs = 0L
        lastTypedWasWordChar = false
        return false
    }

    fun pickSuggestion(word: String, target: InputTarget) {
        val typed = when {
            lang == StripLang.JA && jaRomaji.isNotEmpty() -> jaRomaji.toString()
            lang.isPinyin && pinyinRaw.isNotEmpty() -> pinyinRaw.toString()
            lang.isHangul && !hangul.isEmpty() -> hangul.text()
            else -> composing.toString()
        }
        composing.clear()
        jaRomaji.clear()
        pinyinRaw.clear()
        hangul.clear()
        variations = emptyList()
        selectedSuggestionIndex = -1
        val delimiter = if (lang.isCjkNoSpace) "" else " "
        target.commit(word + delimiter)
        if (!isPasswordField && !lang.isCjkNoSpace && word.length >= 2 && !word.contains(' ')) {
            personal?.learn(lang.code, word)
        }
        recordCommittedWord(word)
        undoRecord = if (typed.isNotEmpty() && !typed.equals(word, ignoreCase = true)) {
            UndoCorrection(typed, word, delimiter)
        } else {
            null
        }
        autoCorrectCandidate = null
        lastTypedWasWordChar = true
        lastSpaceMs = nowMs()
        refreshNextWordPredictions()
    }

    fun forgetSuggestion(word: String): Boolean {
        val removed = personal?.forget(lang.code, word) == true
        if (removed) refreshSuggestions()
        return removed
    }

    fun pickVariation(replacement: String, target: InputTarget) {
        variations = emptyList()
        val splitPrefix = lastSplitPrefix
        val splitSym = lastSplitSymbol
        lastSplitPrefix = null
        lastSplitSymbol = null
        if (lang == StripLang.JA && jaRomaji.isNotEmpty()) {
            jaRomaji.deleteCharAt(jaRomaji.lastIndex)
            if (jaRomaji.isNotEmpty()) {
                val kana = RomajiKanaConverter.toHiragana(jaRomaji.toString(), flushTrailingN = false)
                composing.clear()
                composing.append(kana)
                commitCurrentWord(target, delimiter = replacement, applyAutoCorrect = false)
            } else {
                composing.clear()
                target.setComposing("")
                target.commit(replacement)
                suggestions = emptyList()
                autoCorrectCandidate = null
                selectedSuggestionIndex = -1
                isNextWordPrediction = false
            }
            lastTypedWasWordChar = false
            return
        }
        if (composing.isNotEmpty()) {
            composing.deleteCharAt(composing.lastIndex)
            if (replacement.length == 1 && isWordChar(replacement[0])) {
                composing.append(replacement)
                target.setComposing(composing.toString())
                refreshSuggestions()
            } else {
                if (composing.isNotEmpty()) {
                    commitCurrentWord(target, delimiter = replacement, applyAutoCorrect = false)
                } else {
                    target.setComposing("")
                    target.commit(replacement)
                    suggestions = emptyList()
                    autoCorrectCandidate = null
                    selectedSuggestionIndex = -1
                    isNextWordPrediction = false
                }
                lastTypedWasWordChar = false
                updateAutoCapForPunct(replacement)
            }
        } else {
            if (!isPasswordField && lang != StripLang.JA && replacement.length == 1 && replacement[0].isLetter()) {
                if (splitPrefix != null && splitSym != null && target.textBeforeCursor(splitPrefix.length + splitSym.length) == splitPrefix + splitSym) {
                    target.deleteSurrounding(splitPrefix.length + splitSym.length, 0)
                    composing.append(splitPrefix).append(replacement)
                } else {
                    target.deleteSurrounding(1, 0)
                    composing.append(replacement)
                }
                target.setComposing(composing.toString())
                lastTypedWasWordChar = true
                pendingAutoCap = false
                refreshSuggestions()
            } else {
                target.deleteSurrounding(1, 0)
                target.commit(replacement)
                lastTypedWasWordChar = false
                updateAutoCapForPunct(replacement)
            }
        }
    }

    private fun updateAutoCapForPunct(text: String) {
        if (text.length == 1 && text[0] in SENTENCE_END) {
            if (config.autoCapitalize) pendingAutoCap = true
            previousWord = ""
        } else {
            pendingAutoCap = false
        }
    }

    fun insertDirectText(text: String, target: InputTarget) {
        variations = emptyList()
        val singleLetter = text.length == 1 && text[0].isLetter()
        if (singleLetter) {
            if (shiftState == StickyState.ONE_SHOT) shiftState = StickyState.OFF
            if (pendingAutoCap) pendingAutoCap = false
        }
        val canCompose = !isPasswordField && !lang.isCjkNoSpace && !lang.isHangul && text.length == 1 &&
            (text[0].isLetter() || (composing.isNotEmpty() && isWordChar(text[0])))
        if (canCompose) {
            undoRecord = null
            selectedSuggestionIndex = -1
            isNextWordPrediction = false
            composing.append(text)
            target.setComposing(composing.toString())
            lastTypedWasWordChar = true
            refreshSuggestions()
            return
        }
        if (hasComposing()) {
            commitCurrentWord(target, delimiter = "", applyAutoCorrect = false)
        }
        target.commit(text)
        undoRecord = null
    }

    fun onCursorMovedExternally(target: InputTarget) {
        if (hasComposing()) {
            target.finishComposing()
            composing.clear()
            jaRomaji.clear()
            pinyinRaw.clear()
            hangul.clear()
            suggestions = emptyList()
            autoCorrectCandidate = null
        }
        selectedSuggestionIndex = -1
        isNextWordPrediction = false
        variations = emptyList()
        lastSplitPrefix = null
        lastSplitSymbol = null
        undoRecord = null
    }

    fun onSelectionChanged(textBefore: String) {
        selectedSuggestionIndex = -1
        variations = emptyList()
        lastSplitPrefix = null
        lastSplitSymbol = null
        undoRecord = null
        if (isNextWordPrediction) {
            suggestions = emptyList()
            isNextWordPrediction = false
        }
        pendingAutoCap = !isPasswordField && config.autoCapitalize && shouldCapitalizeAfter(textBefore)
        previousWord = if (isPasswordField || pendingAutoCap) "" else extractPreviousWord(textBefore)
    }

    fun snapshot(
        kumiConnected: Boolean,
        kumiLayer: Int,
        kumiCapsWord: Boolean,
        hasHardwareKeyboard: Boolean = true,
        clipboard: List<ClipboardItem>,
        physicalShiftHeld: Boolean = false,
        physicalCapsLock: Boolean = false,
    ): ImeUiState = ImeUiState(
        visible = config.stripEnabled,
        lang = lang,
        composing = composing.toString(),
        suggestions = if (isPasswordField) emptyList() else suggestions,
        selectedSuggestionIndex = if (isPasswordField) -1 else selectedSuggestionIndex,
        isNextWordPrediction = !isPasswordField && isNextWordPrediction,
        autoHighlightFirst = !isPasswordField && !isNextWordPrediction && (config.autoCorrect || lang.isCjkNoSpace) && autoCorrectCandidate != null &&
            suggestions.firstOrNull() == autoCorrectCandidate,
        variations = variations,
        activeDrawer = activeDrawer,
        emojiQuery = emojiQuery.toString(),
        shiftState = shiftState,
        physicalShiftHeld = physicalShiftHeld,
        effectiveUpper = (physicalShiftHeld xor physicalCapsLock) || shiftState != StickyState.OFF || kumiCapsWord || (pendingAutoCap && composing.isEmpty()),
        altState = StickyState.OFF,
        capsWord = kumiCapsWord,
        kumiConnected = kumiConnected,
        kumiLayer = kumiLayer,
        hasHardwareKeyboard = hasHardwareKeyboard,
        undoAvailable = undoRecord != null,
        isPasswordField = isPasswordField,
        clipboardItems = clipboard,
        snippets = config.snippets,
    )

    private fun onJapaneseCharTyped(rawChar: Char, target: InputTarget): Boolean {
        val lower = rawChar.lowercaseChar()
        if (lower in 'a'..'z' || lower == '\'' || (lower == '-' && jaRomaji.isNotEmpty())) {
            undoRecord = null
            variations = emptyList()
            selectedSuggestionIndex = -1
            isNextWordPrediction = false
            jaRomaji.append(lower)
            val kana = RomajiKanaConverter.toHiragana(jaRomaji.toString(), flushTrailingN = false)
            composing.clear()
            composing.append(kana)
            target.setComposing(kana)
            lastTypedWasWordChar = true
            refreshSuggestions()
            return true
        }
        variations = emptyList()
        if (rawChar == ' ') {
            if (selectedSuggestionIndex in suggestions.indices) {
                pickSuggestion(suggestions[selectedSuggestionIndex], target)
                return true
            }
            if (jaRomaji.isNotEmpty()) {
                commitCurrentWord(target, delimiter = "", applyAutoCorrect = true)
                lastTypedWasWordChar = false
                lastSpaceMs = 0L
                refreshNextWordPredictions()
                return true
            }
            return false
        }
        if (rawChar in JA_PUNCTUATION || rawChar in SENTENCE_END || rawChar in WORD_DELIMITERS) {
            selectedSuggestionIndex = -1
            if (jaRomaji.isNotEmpty()) {
                commitCurrentWord(target, delimiter = rawChar.toString(), applyAutoCorrect = true)
            } else {
                target.commit(rawChar.toString())
            }
            previousWord = ""
            suggestions = emptyList()
            isNextWordPrediction = false
            lastTypedWasWordChar = false
            lastSpaceMs = 0L
            return true
        }
        if (jaRomaji.isNotEmpty()) {
            commitCurrentWord(target, delimiter = "", applyAutoCorrect = false)
        }
        return false
    }

    private fun onPinyinCharTyped(rawChar: Char, target: InputTarget): Boolean {
        val lower = rawChar.lowercaseChar()
        if (lower in 'a'..'z' || (lower == '\'' && pinyinRaw.isNotEmpty())) {
            undoRecord = null
            variations = emptyList()
            selectedSuggestionIndex = -1
            isNextWordPrediction = false
            pinyinRaw.append(lower)
            val formatted = PinyinEngine.formatComposing(pinyinRaw.toString())
            composing.clear()
            composing.append(formatted)
            target.setComposing(formatted)
            lastTypedWasWordChar = true
            refreshSuggestions()
            return true
        }
        variations = emptyList()
        if (rawChar == ' ') {
            if (selectedSuggestionIndex in suggestions.indices) {
                pickSuggestion(suggestions[selectedSuggestionIndex], target)
                return true
            }
            if (pinyinRaw.isNotEmpty()) {
                commitCurrentWord(target, delimiter = "", applyAutoCorrect = true)
                lastTypedWasWordChar = false
                lastSpaceMs = 0L
                refreshNextWordPredictions()
                return true
            }
            return false
        }
        if (rawChar in ZH_PUNCTUATION || rawChar in SENTENCE_END || rawChar in WORD_DELIMITERS) {
            selectedSuggestionIndex = -1
            if (pinyinRaw.isNotEmpty()) {
                commitCurrentWord(target, delimiter = rawChar.toString(), applyAutoCorrect = true)
            } else {
                target.commit(rawChar.toString())
            }
            previousWord = ""
            suggestions = emptyList()
            isNextWordPrediction = false
            lastTypedWasWordChar = false
            lastSpaceMs = 0L
            return true
        }
        if (pinyinRaw.isNotEmpty()) {
            commitCurrentWord(target, delimiter = "", applyAutoCorrect = false)
        }
        return false
    }

    private fun onHangulCharTyped(rawChar: Char, target: InputTarget, isRepeat: Boolean): Boolean {
        if (rawChar in 'a'..'z' || rawChar in 'A'..'Z') {
            undoRecord = null
            variations = emptyList()
            selectedSuggestionIndex = -1
            isNextWordPrediction = false
            if (hangul.feedLatin(rawChar)) {
                val text = hangul.text()
                composing.clear()
                composing.append(text)
                target.setComposing(text)
                lastTypedWasWordChar = true
                refreshSuggestions()
                return true
            }
        }
        variations = emptyList()
        if (rawChar == ' ') {
            if (selectedSuggestionIndex in suggestions.indices) {
                pickSuggestion(suggestions[selectedSuggestionIndex], target)
                return true
            }
            val now = nowMs()
            if (!isRepeat && hangul.isEmpty() && config.doubleSpacePeriod && lastTypedWasWordChar &&
                now - lastSpaceMs in 1..DOUBLE_SPACE_MS && target.textBeforeCursor(2).endsWith(" ")
            ) {
                val before = target.textBeforeCursor(3)
                if (before.length >= 2 && before[before.length - 2].isLetterOrDigit()) {
                    target.deleteSurrounding(1, 0)
                    target.commit(". ")
                    previousWord = ""
                    suggestions = emptyList()
                    isNextWordPrediction = false
                    selectedSuggestionIndex = -1
                    lastSpaceMs = 0L
                    lastTypedWasWordChar = false
                    undoRecord = null
                    return true
                }
            }
            if (!hangul.isEmpty()) {
                commitCurrentWord(target, delimiter = " ", applyAutoCorrect = false)
                lastSpaceMs = now
                lastTypedWasWordChar = true
                refreshNextWordPredictions()
                return true
            }
            lastSpaceMs = now
            return false
        }
        if (rawChar in SENTENCE_END || rawChar in WORD_DELIMITERS) {
            selectedSuggestionIndex = -1
            if (!hangul.isEmpty()) {
                commitCurrentWord(target, delimiter = rawChar.toString(), applyAutoCorrect = false)
            } else {
                target.commit(rawChar.toString())
            }
            if (rawChar in SENTENCE_END) previousWord = ""
            suggestions = emptyList()
            isNextWordPrediction = false
            lastTypedWasWordChar = false
            lastSpaceMs = 0L
            return true
        }
        if (!hangul.isEmpty()) {
            commitCurrentWord(target, delimiter = "", applyAutoCorrect = false)
        }
        return false
    }

    private fun commitCurrentWord(target: InputTarget, delimiter: String, applyAutoCorrect: Boolean) {
        selectedSuggestionIndex = -1
        if (lang == StripLang.JA && jaRomaji.isNotEmpty()) {
            val rawRomaji = jaRomaji.toString()
            val flushedKana = RomajiKanaConverter.toHiragana(rawRomaji, flushTrailingN = true)
            jaRomaji.clear()
            composing.clear()
            val chosen = if (applyAutoCorrect && autoCorrectCandidate != null &&
                !rawRomaji.equals(suppressedAutoCorrectWord, ignoreCase = true)
            ) {
                autoCorrectCandidate!!
            } else {
                flushedKana
            }
            target.commit(chosen + delimiter)
            recordCommittedWord(chosen)
            undoRecord = if (chosen != flushedKana || chosen != rawRomaji) {
                UndoCorrection(rawRomaji, chosen, delimiter)
            } else {
                null
            }
            suppressedAutoCorrectWord = null
            suggestions = emptyList()
            isNextWordPrediction = false
            autoCorrectCandidate = null
            return
        }
        if (lang.isPinyin && pinyinRaw.isNotEmpty()) {
            val raw = pinyinRaw.toString()
            pinyinRaw.clear()
            composing.clear()
            val chosen = if (applyAutoCorrect && autoCorrectCandidate != null &&
                !raw.equals(suppressedAutoCorrectWord, ignoreCase = true)
            ) {
                autoCorrectCandidate!!
            } else {
                raw
            }
            target.commit(chosen + delimiter)
            recordCommittedWord(chosen)
            undoRecord = if (chosen != raw) UndoCorrection(raw, chosen, delimiter) else null
            suppressedAutoCorrectWord = null
            suggestions = emptyList()
            isNextWordPrediction = false
            autoCorrectCandidate = null
            return
        }
        if (lang.isHangul && !hangul.isEmpty()) {
            val text = hangul.text()
            hangul.clear()
            composing.clear()
            target.commit(text + delimiter)
            if (!isPasswordField && text.length >= 2 && text.all(HangulComposer::isHangul)) {
                personal?.learn(lang.code, text)
            }
            recordCommittedWord(text)
            undoRecord = null
            suppressedAutoCorrectWord = null
            suggestions = emptyList()
            isNextWordPrediction = false
            autoCorrectCandidate = null
            return
        }
        val typed = composing.toString()
        composing.clear()
        val chosen = if (applyAutoCorrect && autoCorrectCandidate != null &&
            !typed.equals(suppressedAutoCorrectWord, ignoreCase = true)
        ) {
            autoCorrectCandidate!!
        } else {
            typed
        }
        target.commit(chosen + delimiter)
        undoRecord = if (!chosen.equals(typed, ignoreCase = false)) {
            UndoCorrection(typed, chosen, delimiter)
        } else {
            null
        }
        if (!isPasswordField && chosen == typed && typed.length >= 3 && typed.all { it.isLetter() || it == '\'' }) {
            personal?.learn(lang.code, typed.lowercase())
        }
        recordCommittedWord(chosen)
        suppressedAutoCorrectWord = null
        suggestions = emptyList()
        isNextWordPrediction = false
        autoCorrectCandidate = null
    }

    private fun recordCommittedWord(word: String) {
        val clean = word.trim()
        if (clean.isEmpty() || clean.contains(' ')) return
        if (!isPasswordField && previousWord.isNotEmpty()) {
            personal?.learnBigram(lang.code, previousWord, if (lang.isCjkNoSpace || lang.isHangul) clean else clean.lowercase())
        }
        previousWord = clean
    }

    private fun refreshNextWordPredictions() {
        selectedSuggestionIndex = -1
        if (isPasswordField || hasComposing() || previousWord.isEmpty()) {
            isNextWordPrediction = false
            return
        }
        val next = nextWordPredictor.predict(previousWord, lang, capitalize = pendingAutoCap, limit = 3)
        suggestions = next
        isNextWordPrediction = next.isNotEmpty()
        autoCorrectCandidate = null
    }

    private fun refreshSuggestions() {
        selectedSuggestionIndex = -1
        isNextWordPrediction = false
        if (isPasswordField) {
            suggestions = emptyList()
            autoCorrectCandidate = null
            return
        }
        if (lang == StripLang.JA) {
            if (jaRomaji.isEmpty()) {
                suggestions = emptyList()
                autoCorrectCandidate = null
                return
            }
            val raw = jaRomaji.toString()
            val kana = RomajiKanaConverter.toHiragana(raw, flushTrailingN = true)
            val kata = RomajiKanaConverter.hiraganaToKatakana(kana)
            val hits = jaDictProvider()?.lookup(kana, limit = 6).orEmpty()
            val out = LinkedHashSet<String>(4)
            hits.take(2).forEach { out.add(it) }
            out.add(kana)
            if (kata != kana) out.add(kata)
            hits.drop(2).forEach { if (out.size < 4) out.add(it) }
            suggestions = out.take(3)
            val top = hits.firstOrNull() ?: kana
            autoCorrectCandidate = if (raw.equals(suppressedAutoCorrectWord, ignoreCase = true)) kana else top
            return
        }
        if (lang.isPinyin) {
            if (pinyinRaw.isEmpty()) {
                suggestions = emptyList()
                autoCorrectCandidate = null
                return
            }
            val raw = pinyinRaw.toString()
            val hits = PinyinEngine.lookup(raw, pinyinDictProvider(lang), limit = 6)
            val out = LinkedHashSet<String>(4)
            hits.forEach { if (out.size < 3) out.add(it) }
            if (out.isEmpty()) out.add(raw)
            suggestions = out.take(3)
            val top = hits.firstOrNull() ?: raw
            autoCorrectCandidate = if (raw.equals(suppressedAutoCorrectWord, ignoreCase = true)) raw else top
            return
        }
        val word = if (lang.isHangul) hangul.text() else composing.toString()
        if (word.isEmpty()) {
            suggestions = emptyList()
            autoCorrectCandidate = null
            return
        }
        val boost = if (previousWord.isNotEmpty()) nextWordPredictor.predictedSet(previousWord, lang) else emptySet()
        val result = engine.suggest(
            rawInput = word,
            lang = lang.code,
            dict = dictProvider(lang),
            snippets = config.snippets,
            maxSuggestions = 3,
            nextWordBoost = boost,
        )
        suggestions = result.suggestions
        autoCorrectCandidate = if (lang.isHangul || word.equals(suppressedAutoCorrectWord, ignoreCase = true)) null else result.autoCorrectCandidate
    }

    private fun extractPreviousWord(before: String): String {
        val trimmed = before.trimEnd()
        if (trimmed.isEmpty()) return ""
        val last = trimmed.last()
        if (last in SENTENCE_END || last in WORD_DELIMITERS) return ""
        val end = trimmed.length
        var start = end
        while (start > 0 && (trimmed[start - 1].isLetter() || trimmed[start - 1] == '\'' || trimmed[start - 1] == '-')) {
            start--
        }
        return if (start < end) trimmed.substring(start, end) else ""
    }

    private fun detectLanguageFromChar(ch: Char) {
        if (!ch.isLetter()) return
        val lower = ch.lowercaseChar()
        when {
            lower in UKRAINIAN_ONLY -> lang = StripLang.UK
            lower in '\u0400'..'\u04FF' && lang.isLatin -> lang = StripLang.RU
            lower in 'a'..'z' && lang.isCyrillic -> lang = config.enabledLangs.firstOrNull { it.isLatin } ?: StripLang.EN
        }
    }

    private fun isWordChar(ch: Char): Boolean = ch.isLetter() || ch == '\'' || (ch == '-' && composing.isNotEmpty())

    private fun shouldCapitalizeAfter(before: String): Boolean {
        val trimmed = before.trimEnd()
        if (trimmed.isEmpty()) return true
        return trimmed.last() in SENTENCE_END
    }

    companion object {
        private const val DOUBLE_SPACE_MS = 450L
        private const val DOUBLE_TAP_MS = 350L
        private val SENTENCE_END = setOf('.', '!', '?')
        private val WORD_DELIMITERS = setOf(',', ';', ':', ')', ']', '}', '"')
        private val JA_PUNCTUATION = setOf('。', '、', '？', '！', '・', '「', '」', '（', '）')
        private val ZH_PUNCTUATION = setOf('，', '。', '、', '？', '！', '：', '；', '「', '」', '（', '）', '“', '”')
        private val UKRAINIAN_ONLY = setOf('ґ', 'є', 'і', 'ї')
    }
}
