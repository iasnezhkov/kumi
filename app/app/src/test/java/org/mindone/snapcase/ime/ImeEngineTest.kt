// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.mindone.snapcase.ime.dict.JapaneseDict
import org.mindone.snapcase.ime.dict.SnapDictBinary
import org.mindone.snapcase.ime.dict.SuggestionEngine

class ImeEngineTest {
    private class FakeTarget : InputTarget {
        val committed = StringBuilder()
        var activeComposing: String = ""

        override fun setComposing(text: String) {
            activeComposing = text
        }

        override fun commit(text: String) {
            activeComposing = ""
            committed.append(text)
        }

        override fun finishComposing() {
            committed.append(activeComposing)
            activeComposing = ""
        }

        override fun deleteSurrounding(before: Int, after: Int) {
            val drop = before.coerceAtMost(committed.length)
            committed.delete(committed.length - drop, committed.length)
        }

        override fun textBeforeCursor(length: Int): String =
            committed.takeLast(length).toString()
    }

    private fun loadBundledDict(code: String): SnapDictBinary {
        val file = File("src/main/assets/dict/$code.snapdict")
        assertTrue("missing bundled dictionary ${file.absolutePath}", file.isFile)
        return file.inputStream().use(SnapDictBinary::load)
    }

    @Test
    fun bundledDictionariesLoadAndSuggestRussian36KeyAndEnglishTypos() {
        val en = loadBundledDict("en")
        val ru = loadBundledDict("ru")
        val uk = loadBundledDict("uk")
        assertTrue(en.count >= 10_000)
        assertTrue(ru.count >= 10_000)
        assertTrue(uk.count >= 10_000)

        val engine = SuggestionEngine()

        val ruYo = engine.suggest("елка", "ru", ru)
        assertTrue("expected ёлка for елка, got ${ruYo.suggestions}", ruYo.suggestions.any { it == "ёлка" || it == "елка" })

        val ruTypo = engine.suggest("привте", "ru", ru)
        assertEquals("привет", ruTypo.suggestions.firstOrNull())

        val ruEto = engine.suggest("ето", "ru", ru)
        assertTrue("expected это for ето, got ${ruEto.suggestions}", ruEto.suggestions.contains("это"))

        val enTypo = engine.suggest("helo", "en", en)
        assertTrue("expected hello in ${enTypo.suggestions}", enTypo.suggestions.contains("hello"))
        assertTrue("expected help in ${enTypo.suggestions}", enTypo.suggestions.contains("help"))

        val enDouble = engine.suggest("helllo", "en", en)
        assertEquals("hello", enDouble.suggestions.firstOrNull())

        val enCase = engine.suggest("Teh", "en", en)
        assertEquals("The", enCase.suggestions.firstOrNull())

        repeat(5) {
            engine.suggest("priv", "en", en)
            engine.suggest("прив", "ru", ru)
        }
        val t0 = System.nanoTime()
        repeat(50) {
            engine.suggest("priv", "en", en)
            engine.suggest("прив", "ru", ru)
        }
        val avgMs = (System.nanoTime() - t0) / 100.0 / 1_000_000.0
        assertTrue("lookup too slow: $avgMs ms", avgMs < 50.0)
    }

    @Test
    fun compileFromFrequencyTextRoundTripsCleanly() {
        val bytes = SnapDictBinary.compileFromFrequencyText("hello 5000\nworld 2000\nhello 100\n", maxWords = 100, cyrillic = false)
        val dict = SnapDictBinary.fromBytes(bytes)
        assertEquals(2, dict.count)
        val hits = dict.prefixMatches("hel")
        assertEquals("hello", hits.first().word)
    }

    @Test
    fun autoCorrectUndoOnBackspaceAndDoubleSpacePeriod() {
        val en = loadBundledDict("en")
        val ru = loadBundledDict("ru")
        var clock = 1_000L
        val processor = ImeKeyProcessor(
            engine = SuggestionEngine(),
            dictProvider = { lang -> if (lang == StripLang.RU) ru else en },
            nowMs = { clock },
        )
        processor.updateConfig(ImeConfig(autoCorrect = true, autoCapitalize = true, doubleSpacePeriod = true))
        val target = FakeTarget()
        processor.startInput(passwordField = false, textBefore = "")

        "helllo".forEach { processor.onCharTyped(it, physicalShifted = false, target) }
        assertEquals("Helllo", target.activeComposing)

        processor.onCharTyped(' ', physicalShifted = false, target)
        assertEquals("Hello ", target.committed.toString())

        assertTrue(processor.onBackspace(target))
        assertEquals("", target.committed.toString())
        assertEquals("Helllo", target.activeComposing)

        processor.onCharTyped(' ', physicalShifted = false, target)
        assertEquals("Helllo ", target.committed.toString())

        clock += 200L
        assertTrue(processor.onCharTyped(' ', physicalShifted = false, target))
        assertEquals("Helllo. ", target.committed.toString())

        "привте".forEach { processor.onCharTyped(it, physicalShifted = false, target) }
        assertEquals("Привте", target.activeComposing)
        processor.onCharTyped(' ', physicalShifted = false, target)
        assertEquals("Helllo. Привет ", target.committed.toString())
    }

    @Test
    fun snippetsAndLongPressVariationsAndPasswordFieldWork() {
        val en = loadBundledDict("en")
        val ru = loadBundledDict("ru")
        val processor = ImeKeyProcessor(
            engine = SuggestionEngine(),
            dictProvider = { lang -> if (lang == StripLang.RU) ru else en },
        )
        val target = FakeTarget()
        processor.startInput(passwordField = false, textBefore = "ok ")

        "omw".forEach { processor.onCharTyped(it, physicalShifted = false, target) }
        processor.onCharTyped(' ', physicalShifted = false, target)
        assertEquals("On my way! ", target.committed.toString())

        processor.setLanguage(StripLang.RU)
        processor.onCharTyped('е', physicalShifted = false, target)
        assertTrue(processor.onLongPressRepeat('е', target, keyCode = android.view.KeyEvent.KEYCODE_T, compactCaseKeyboard = true))
        assertEquals("On my way! 5", target.committed.toString())
        assertEquals("", target.activeComposing)
        var state = processor.snapshot(false, 0, false, true, emptyList())
        assertTrue(state.variations.contains("ё"))
        processor.pickVariation("ё", target)
        assertEquals("On my way! ", target.committed.toString())
        assertEquals("ё", target.activeComposing)

        processor.onCharTyped(' ', physicalShifted = false, target)
        processor.onCharTyped('н', physicalShifted = false, target)
        assertTrue(processor.onLongPressRepeat('н', target, keyCode = android.view.KeyEvent.KEYCODE_Y, compactCaseKeyboard = true))
        assertEquals("On my way! ё 6", target.committed.toString())

        processor.updateConfig(ImeConfig(longPressBehavior = LongPressBehavior.VARIATIONS))
        processor.setLanguage(StripLang.EN)
        processor.onCharTyped('y', physicalShifted = false, target)
        assertTrue(processor.onLongPressRepeat('y', target, keyCode = android.view.KeyEvent.KEYCODE_Y, compactCaseKeyboard = true))
        state = processor.snapshot(false, 0, false, true, emptyList())
        assertEquals("6", state.variations.firstOrNull())
        processor.pickVariation("6", target)
        assertEquals("On my way! ё 66", target.committed.toString())

        processor.updateConfig(ImeConfig(longPressBehavior = LongPressBehavior.ALT_SYMBOL))
        processor.startInput(passwordField = true, textBefore = "")
        assertFalse(processor.onCharTyped('a', physicalShifted = false, target))
        target.committed.clear()
        target.commit("y")
        assertTrue(processor.onLongPressRepeat('y', target, keyCode = android.view.KeyEvent.KEYCODE_Y, compactCaseKeyboard = true))
        assertEquals("6", target.committed.toString())
    }

    @Test
    fun physicalLayoutTranslatorAndEnabledLanguageCycleWork() {
        assertEquals('й', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_Q, false, 'q'.code, StripLang.RU))
        assertEquals('Й', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_Q, true, 'Q'.code, StripLang.RU))
        assertEquals('q', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_Q, false, 'й'.code, StripLang.EN))
        assertEquals('і', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_S, false, 's'.code, StripLang.UK))
        assertEquals('.', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_PERIOD, false, '.'.code, StripLang.RU, compactCaseKeyboard = true))
        assertEquals(',', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_PERIOD, true, '>'.code, StripLang.RU, compactCaseKeyboard = true))
        assertEquals('ю', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_PERIOD, false, '.'.code, StripLang.RU, compactCaseKeyboard = false))
        assertEquals('Ю', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_PERIOD, true, '>'.code, StripLang.RU, compactCaseKeyboard = false))
        assertEquals('.', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_SLASH, false, '/'.code, StripLang.RU, compactCaseKeyboard = false))
        assertEquals(',', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_SLASH, true, '?'.code, StripLang.RU, compactCaseKeyboard = false))
        assertEquals('>', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_PERIOD, true, '>'.code, StripLang.EN, compactCaseKeyboard = false))
        assertEquals('A', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_A, false, 'a'.code, StripLang.EN, capsLock = true))
        assertEquals('a', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_A, true, 'A'.code, StripLang.EN, capsLock = true))
        assertEquals('Ф', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_A, false, 'a'.code, StripLang.RU, capsLock = true))
        assertEquals('。', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_PERIOD, false, '.'.code, StripLang.JA, literalPunctuation = false))
        assertEquals('.', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_PERIOD, false, '.'.code, StripLang.JA, literalPunctuation = true))
        assertEquals('б', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_COMMA, false, ','.code, StripLang.RU, compactCaseKeyboard = false))
        assertEquals(',', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_COMMA, false, ','.code, StripLang.RU, compactCaseKeyboard = true))
        assertEquals(';', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_SEMICOLON, false, ';'.code, StripLang.RU, compactCaseKeyboard = true))
        assertEquals('\'', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_APOSTROPHE, false, '\''.code, StripLang.RU, compactCaseKeyboard = true))
        assertEquals('[', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_LEFT_BRACKET, false, '['.code, StripLang.RU, compactCaseKeyboard = true))
        assertEquals(']', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_RIGHT_BRACKET, false, ']'.code, StripLang.UK, compactCaseKeyboard = true))
        assertEquals('і', PhysicalLayoutTranslator.translate(android.view.KeyEvent.KEYCODE_S, false, 's'.code, StripLang.UK, compactCaseKeyboard = true))

        val processor = ImeKeyProcessor(SuggestionEngine(), { null })
        processor.updateConfig(ImeConfig(enabledLangs = listOf(StripLang.EN, StripLang.RU)))
        assertEquals(StripLang.EN, processor.currentLanguage())
        assertEquals(StripLang.RU, processor.cycleLanguage())
        assertEquals(StripLang.EN, processor.cycleLanguage())
    }

    @Test
    fun shiftEffectiveUpperAndDirectExtraLetterComposeWork() {
        val ru = loadBundledDict("ru")
        val processor = ImeKeyProcessor(SuggestionEngine(), { lang -> if (lang == StripLang.RU) ru else null })
        processor.updateConfig(ImeConfig(autoCapitalize = true, stickyShift = true))
        val target = FakeTarget()
        processor.startInput(passwordField = false, textBefore = "")
        processor.setLanguage(StripLang.RU)

        var snap = processor.snapshot(false, 0, false, true, emptyList(), physicalShiftHeld = false)
        assertTrue(snap.effectiveUpper)
        assertFalse(snap.physicalShiftHeld)

        processor.insertDirectText("Х", target)
        assertEquals("Х", target.activeComposing)
        snap = processor.snapshot(false, 0, false, true, emptyList(), physicalShiftHeld = false)
        assertFalse(snap.effectiveUpper)

        "орошо".forEach { processor.onCharTyped(it, physicalShifted = false, target, autoDetectLang = false) }
        assertEquals("Хорошо", target.activeComposing)
        processor.onCharTyped(' ', physicalShifted = false, target, autoDetectLang = false)
        assertEquals("Хорошо ", target.committed.toString())

        snap = processor.snapshot(false, 0, false, true, emptyList(), physicalShiftHeld = true)
        assertTrue(snap.effectiveUpper)
        assertTrue(snap.physicalShiftHeld)

        processor.onShiftTap()
        snap = processor.snapshot(false, 0, false, true, emptyList(), physicalShiftHeld = false)
        assertEquals(StickyState.ONE_SHOT, snap.shiftState)
        assertTrue(snap.effectiveUpper)

        processor.insertDirectText("Ё", target)
        assertEquals("Ё", target.activeComposing)
        snap = processor.snapshot(false, 0, false, true, emptyList(), physicalShiftHeld = false)
        assertEquals(StickyState.OFF, snap.shiftState)
        assertFalse(snap.effectiveUpper)
    }

    @Test
    fun japaneseRomajiKanaKanjiConversionAndUndoWork() {
        assertEquals("にほんご", RomajiKanaConverter.toHiragana("nihongo"))
        assertEquals("きって", RomajiKanaConverter.toHiragana("kitte"))
        assertEquals("こんにちは", RomajiKanaConverter.toHiragana("konnichiha"))
        assertEquals("こんな", RomajiKanaConverter.toHiragana("konna"))
        assertEquals("おんな", RomajiKanaConverter.toHiragana("onnna"))
        assertEquals("にほん", RomajiKanaConverter.toHiragana("nihonn"))
        assertEquals("きんようび", RomajiKanaConverter.toHiragana("kin'youbi"))
        assertEquals("がっこう", RomajiKanaConverter.toHiragana("gakkou"))
        assertEquals("かんたん", RomajiKanaConverter.toHiragana("kantan", flushTrailingN = true))
        assertEquals("ニホンゴ", RomajiKanaConverter.hiraganaToKatakana("にほんご"))

        val jaFile = File("src/main/assets/dict/ja.snapja")
        assertTrue("missing bundled Japanese dictionary ${jaFile.absolutePath}", jaFile.isFile)
        val jaDict = jaFile.inputStream().use(JapaneseDict::load)
        assertTrue(jaDict.count >= 10_000)
        assertTrue("expected 日本語 in ${jaDict.lookup("にほんご")}", jaDict.lookup("にほんご").contains("日本語"))
        assertTrue("expected 東京 in ${jaDict.lookup("とうきょう")}", jaDict.lookup("とうきょう").contains("東京"))

        val processor = ImeKeyProcessor(
            engine = SuggestionEngine(),
            dictProvider = { null },
            jaDictProvider = { jaDict },
        )
        val target = FakeTarget()
        processor.startInput(passwordField = false, textBefore = "")
        processor.setLanguage(StripLang.JA)

        "nihongo".forEach { processor.onCharTyped(it, physicalShifted = false, target, autoDetectLang = false) }
        assertEquals("にほんご", target.activeComposing)
        val snap = processor.snapshot(false, 0, false, true, emptyList())
        assertTrue("expected 日本語 in ${snap.suggestions}", snap.suggestions.contains("日本語"))

        processor.onCharTyped(' ', physicalShifted = false, target, autoDetectLang = false)
        assertEquals("日本語", target.committed.toString())

        assertTrue(processor.onBackspace(target))
        assertEquals("", target.committed.toString())
        assertEquals("にほんご", target.activeComposing)

        assertTrue(processor.onEnter(target))
        assertEquals("にほんご", target.committed.toString())
        assertEquals("", target.activeComposing)
    }

    @Test
    fun nextWordPredictionAndKeyboardCandidateSelectionWork() {
        val en = loadBundledDict("en")
        val ru = loadBundledDict("ru")
        val tempDir = File(System.getProperty("java.io.tmpdir"), "kumi-ime-test-${System.nanoTime()}").apply { mkdirs() }
        try {
            val personal = org.mindone.snapcase.ime.dict.PersonalDictionary(tempDir)
            val processor = ImeKeyProcessor(
                engine = SuggestionEngine(personal),
                dictProvider = { lang -> if (lang == StripLang.RU) ru else en },
                personal = personal,
            )
            val target = FakeTarget()
            processor.startInput(passwordField = false, textBefore = "")
            processor.setLanguage(StripLang.RU)

            "привет".forEach { processor.onCharTyped(it, physicalShifted = false, target, autoDetectLang = false) }
            processor.onCharTyped(' ', physicalShifted = false, target, autoDetectLang = false)
            var snap = processor.snapshot(false, 0, false, true, emptyList())
            assertTrue("expected next-word prediction after привет", snap.isNextWordPrediction)
            assertEquals("как", snap.suggestions.firstOrNull())

            assertTrue(processor.acceptSuggestionAt(0, target))
            assertEquals("Привет как ", target.committed.toString())
            snap = processor.snapshot(false, 0, false, true, emptyList())
            assertTrue(snap.isNextWordPrediction)
            assertEquals("дела", snap.suggestions.firstOrNull())

            assertTrue(processor.moveSuggestionSelection(1))
            assertTrue(processor.moveSuggestionSelection(1))
            snap = processor.snapshot(false, 0, false, true, emptyList())
            assertEquals(1, snap.selectedSuggestionIndex)
            val secondWord = snap.suggestions[1]
            assertTrue(processor.onEnter(target))
            assertEquals("Привет как $secondWord ", target.committed.toString())

            personal.learnBigram("ru", "кофе", "утром")
            processor.startInput(passwordField = false, textBefore = "кофе ")
            snap = processor.snapshot(false, 0, false, true, emptyList())
            assertEquals("утром", snap.suggestions.firstOrNull())
        } finally {
            tempDir.deleteRecursively()
        }
    }

    @Test
    fun imeRegressionsNumericLongPressMidWordVariationRepeatSpaceAndPersonalWordWork() {
        val en = loadBundledDict("en")
        val ru = loadBundledDict("ru")
        val tempDir = File(System.getProperty("java.io.tmpdir"), "kumi-ime-reg-${System.nanoTime()}").apply { mkdirs() }
        try {
            val personal = org.mindone.snapcase.ime.dict.PersonalDictionary(tempDir)
            personal.learn("en", "kumi")
            var clock = 1_000L
            val processor = ImeKeyProcessor(
                engine = SuggestionEngine(personal),
                dictProvider = { lang -> if (lang == StripLang.RU) ru else en },
                personal = personal,
                nowMs = { clock },
            )
            processor.updateConfig(ImeConfig(autoCorrect = true, autoCapitalize = true, doubleSpacePeriod = true))
            val target = FakeTarget()

            processor.startInput(passwordField = true, textBefore = "12")
            target.committed.append("12")
            assertTrue(processor.onLongPressRepeat('w', target, keyCode = android.view.KeyEvent.KEYCODE_W, compactCaseKeyboard = true, numericField = true))
            assertEquals("122", target.committed.toString())

            target.committed.clear()
            processor.startInput(passwordField = false, textBefore = "")
            processor.setLanguage(StripLang.EN)
            "kumi".forEach { processor.onCharTyped(it, physicalShifted = false, target, autoDetectLang = false) }
            processor.onCharTyped(' ', physicalShifted = false, target, autoDetectLang = false)
            assertEquals("Kumi ", target.committed.toString())

            clock += 200L
            assertFalse(processor.onCharTyped(' ', physicalShifted = false, target, autoDetectLang = false, isRepeat = true))
            assertEquals("Kumi ", target.committed.toString())

            target.committed.clear()
            processor.startInput(passwordField = false, textBefore = "ok ")
            processor.setLanguage(StripLang.RU)
            "пре".forEach { processor.onCharTyped(it, physicalShifted = false, target, autoDetectLang = false) }
            assertTrue(processor.onLongPressRepeat('е', target, keyCode = android.view.KeyEvent.KEYCODE_T, compactCaseKeyboard = true))
            assertEquals("пр5", target.committed.toString())
            processor.pickVariation("ё", target)
            assertEquals("", target.committed.toString())
            assertEquals("прё", target.activeComposing)

            target.committed.clear()
            target.activeComposing = ""
            processor.startInput(passwordField = false, textBefore = "")
            processor.setLanguage(StripLang.JA)
            "ni".forEach { processor.onCharTyped(it, physicalShifted = false, target, autoDetectLang = false) }
            assertTrue(processor.onLongPressRepeat('i', target, keyCode = android.view.KeyEvent.KEYCODE_I, compactCaseKeyboard = true))
            processor.onCharTyped('a', physicalShifted = false, target, autoDetectLang = false)
            assertEquals("あ", target.activeComposing)

            target.committed.clear()
            target.activeComposing = ""
            processor.startInput(passwordField = false, textBefore = "Hi. ")
            target.committed.append("Hi. ")
            processor.setLanguage(StripLang.EN)
            processor.onCharTyped('a', physicalShifted = false, target, autoDetectLang = false)
            assertEquals("A", target.activeComposing)
            assertTrue(processor.onBackspace(target))
            assertTrue(processor.snapshot(false, 0, false, true, emptyList()).effectiveUpper)

            target.committed.clear()
            target.activeComposing = ""
            processor.startInput(passwordField = false, textBefore = "")
            "helllo".forEach { processor.onCharTyped(it, physicalShifted = false, target, autoDetectLang = false) }
            processor.onCharTyped(' ', physicalShifted = false, target, autoDetectLang = false)
            assertEquals("Hello ", target.committed.toString())
            processor.cycleLanguage()
            assertFalse(processor.onBackspace(target))
        } finally {
            tempDir.deleteRecursively()
        }
    }
}
