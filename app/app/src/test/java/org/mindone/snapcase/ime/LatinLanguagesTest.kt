// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import android.view.KeyEvent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.mindone.snapcase.ime.dict.DictionaryPackManager
import org.mindone.snapcase.ime.dict.NextWordPredictor
import org.mindone.snapcase.ime.dict.SnapDictBinary
import org.mindone.snapcase.ime.dict.SuggestionEngine

class LatinLanguagesTest {
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

        override fun textBeforeCursor(length: Int): String = committed.takeLast(length).toString()
    }

    private val newLatin = listOf(
        StripLang.ES, StripLang.DE, StripLang.FR, StripLang.IT, StripLang.PT, StripLang.PL, StripLang.NL,
        StripLang.TR, StripLang.CS, StripLang.SV, StripLang.FI, StripLang.RO, StripLang.HU,
    )

    private fun spanishDict(): SnapDictBinary {
        val text = "de 90000\nhola 50000\nespaña 30000\nespañol 20000\nesperar 15000\nniño 12000\ncorazón 9000\n"
        return SnapDictBinary.fromBytes(SnapDictBinary.compileFromFrequencyText(text, maxWords = 100))
    }

    @Test
    fun typingInLatinLanguagesNeverSnapsBackToEnglish() {
        val es = spanishDict()
        for (lang in newLatin) {
            val processor = ImeKeyProcessor(SuggestionEngine(), { if (it == lang) es else null })
            processor.updateConfig(ImeConfig(enabledLangs = listOf(StripLang.EN, lang), autoCapitalize = false))
            val target = FakeTarget()
            processor.startInput(passwordField = false, textBefore = "")
            processor.setLanguage(lang)
            "hola".forEach { processor.onCharTyped(it, physicalShifted = false, target, autoDetectLang = true) }
            assertEquals(lang, processor.currentLanguage())
        }
    }

    @Test
    fun cyrillicAndLatinDetectionStillSwitchesBetweenScripts() {
        val processor = ImeKeyProcessor(SuggestionEngine(), { null })
        processor.updateConfig(ImeConfig(enabledLangs = listOf(StripLang.ES, StripLang.RU), autoCapitalize = false))
        val target = FakeTarget()
        processor.startInput(passwordField = false, textBefore = "")
        processor.setLanguage(StripLang.ES)
        processor.onCharTyped('ж', physicalShifted = false, target, autoDetectLang = true)
        assertEquals(StripLang.RU, processor.currentLanguage())
        processor.onCharTyped(' ', physicalShifted = false, target, autoDetectLang = true)
        processor.onCharTyped('q', physicalShifted = false, target, autoDetectLang = true)
        assertEquals(StripLang.ES, processor.currentLanguage())
    }

    @Test
    fun spanishAccentFoldingSuggestsAndAutoCorrectsEspana() {
        val es = spanishDict()
        val result = SuggestionEngine().suggest("espana", "es", es)
        assertTrue(result.suggestions.toString(), "españa" in result.suggestions)
        assertEquals("españa", result.autoCorrectCandidate)

        val processor = ImeKeyProcessor(SuggestionEngine(), { if (it == StripLang.ES) es else null })
        processor.updateConfig(ImeConfig(enabledLangs = listOf(StripLang.ES), autoCapitalize = false))
        val target = FakeTarget()
        processor.startInput(passwordField = false, textBefore = "")
        processor.setLanguage(StripLang.ES)
        "espana".forEach { processor.onCharTyped(it, physicalShifted = false, target, autoDetectLang = true) }
        processor.onCharTyped(' ', physicalShifted = false, target, autoDetectLang = true)
        assertEquals("españa ", target.committed.toString())
        assertEquals(StripLang.ES, processor.currentLanguage())
    }

    @Test
    fun foldKeyHandlesLatinSpecificLetters() {
        assertEquals("espana", SnapDictBinary.foldKey("España"))
        assertEquals("francais", SnapDictBinary.foldKey("français"))
        assertEquals("lodz", SnapDictBinary.foldKey("Łódź"))
        assertEquals("strasse", SnapDictBinary.foldKey("Straße"))
        assertEquals("gunes", SnapDictBinary.foldKey("güneş"))
        assertEquals("agac", SnapDictBinary.foldKey("ağaç"))
        assertEquals("istanbul", SnapDictBinary.foldKey("İstanbul"))
        assertEquals("kiz", SnapDictBinary.foldKey("kız"))
        assertEquals("cestina", SnapDictBinary.foldKey("čeština"))
        assertEquals("romana", SnapDictBinary.foldKey("română"))
        assertEquals("magyar", SnapDictBinary.foldKey("Magyar"))
        assertEquals("oren", SnapDictBinary.foldKey("Ören"))
    }

    @Test
    fun spanishQuickExtrasLongPressVariationsAndShiftUppercasing() {
        val extras = LocaleVariations.extrasFor(StripLang.ES)
        listOf("ñ", "á", "é", "í", "ó", "ú", "ü", "¡", "¿", "«", "»", "€").forEach { assertTrue(it, it in extras) }
        assertEquals("Ñ", LocaleVariations.shiftExtra("ñ", true))
        assertEquals("Á", LocaleVariations.shiftExtra("á", true))
        assertEquals("¡", LocaleVariations.shiftExtra("¡", true))
        assertEquals("ñ", LocaleVariations.shiftExtra("ñ", false))
        assertEquals("ß", LocaleVariations.shiftExtra("ß", true))
        assertEquals("ñ", LocaleVariations.forChar('n', StripLang.ES).first())
        assertEquals("Ñ", LocaleVariations.forChar('N', StripLang.ES).first())
        assertEquals("á", LocaleVariations.forChar('a', StripLang.ES).first())
        assertEquals("é", LocaleVariations.forChar('e', StripLang.ES).first())
        assertEquals("¿", LocaleVariations.forChar('?', StripLang.ES).first())
        assertEquals("¡", LocaleVariations.forChar('!', StripLang.ES).first())
        assertTrue("¿" in LocaleVariations.forChar('.', StripLang.ES))
    }

    @Test
    fun everyLatinLanguageHasItsSpecificLettersInExtrasAndLongPress() {
        val expected = mapOf(
            StripLang.DE to listOf("ä", "ö", "ü", "ß", "„", "“", "€", "§", "—", "…"),
            StripLang.FR to listOf("é", "è", "ê", "ë", "à", "â", "ç", "ù", "û", "ô", "œ", "«", "»"),
            StripLang.IT to listOf("à", "è", "é", "ì", "ò", "ó", "ù", "€", "«", "»"),
            StripLang.PT to listOf("ã", "õ", "ç", "á", "é", "í", "ó", "ú", "â", "ê", "ô", "à"),
            StripLang.PL to listOf("ą", "ć", "ę", "ł", "ń", "ó", "ś", "ź", "ż"),
            StripLang.TR to listOf("ç", "ğ", "ı", "İ", "ö", "ş", "ü"),
            StripLang.CS to listOf("á", "č", "ď", "é", "ě", "í", "ň", "ó", "ř", "š", "ť", "ú", "ů", "ý", "ž"),
            StripLang.SV to listOf("å", "ä", "ö"),
            StripLang.FI to listOf("ä", "ö", "å"),
            StripLang.RO to listOf("ă", "â", "î", "ș", "ț"),
            StripLang.HU to listOf("á", "é", "í", "ó", "ö", "ő", "ú", "ü", "ű"),
        )
        for ((lang, letters) in expected) {
            val extras = LocaleVariations.extrasFor(lang)
            letters.forEach { assertTrue("$lang $it", it in extras) }
        }
        assertEquals("ä", LocaleVariations.forChar('a', StripLang.DE).first())
        assertEquals("ß", LocaleVariations.forChar('s', StripLang.DE).first())
        assertEquals("ç", LocaleVariations.forChar('c', StripLang.FR).first())
        assertEquals("ã", LocaleVariations.forChar('a', StripLang.PT).first())
        assertEquals("ł", LocaleVariations.forChar('l', StripLang.PL).first())
        assertEquals("ğ", LocaleVariations.forChar('g', StripLang.TR).first())
        assertEquals("ř", LocaleVariations.forChar('r', StripLang.CS).first())
        assertEquals("å", LocaleVariations.forChar('a', StripLang.SV).first())
        assertEquals(listOf("ó", "ö", "ő"), LocaleVariations.forChar('o', StripLang.HU).take(3))
        assertEquals("ș", LocaleVariations.forChar('s', StripLang.RO).first())
        assertEquals("Ć", LocaleVariations.forChar('C', StripLang.PL).first())
        newLatin.forEach { assertTrue(it.name, LocaleVariations.extrasFor(it).isNotEmpty()) }
    }

    @Test
    fun physicalKeysMapThroughQwertyForEveryLatinLanguage() {
        for (lang in newLatin) {
            assertEquals('n', PhysicalLayoutTranslator.translate(KeyEvent.KEYCODE_N, false, 'n'.code, lang))
            assertEquals('N', PhysicalLayoutTranslator.translate(KeyEvent.KEYCODE_N, true, 'N'.code, lang))
            assertEquals('Q', PhysicalLayoutTranslator.translate(KeyEvent.KEYCODE_Q, false, 'q'.code, lang, capsLock = true))
            assertEquals('>', PhysicalLayoutTranslator.translate(KeyEvent.KEYCODE_PERIOD, true, '>'.code, lang))
            assertEquals('ñ', PhysicalLayoutTranslator.translate(KeyEvent.KEYCODE_SEMICOLON, false, 'ñ'.code, lang))
        }
    }

    @Test
    fun stripLangFromCodeAndCatalogParity() {
        newLatin.forEach { assertEquals(it, StripLang.fromCode(it.code)) }
        assertEquals(StripLang.ES, StripLang.fromCode("ES"))
        assertEquals(StripLang.EN, StripLang.fromCode("xx"))
        assertEquals(StripLang.EN, StripLang.fromCode(null))
        assertEquals(17, StripLang.entries.size)
        val catalog = DictionaryPackManager.CATALOG
        assertEquals(StripLang.entries.map { it.code }.toSet(), catalog.map { it.code }.toSet())
        assertEquals(catalog.size, catalog.map { it.code }.toSet().size)
        assertEquals(setOf("en", "ru", "uk", "ja"), catalog.filter { it.bundled }.map { it.code }.toSet())
        assertEquals(setOf("ja"), catalog.filter { it.japanese }.map { it.code }.toSet())
        assertEquals(setOf("ru", "uk"), catalog.filter { it.cyrillic }.map { it.code }.toSet())
        val downloadable = catalog.filter { !it.bundled }
        assertEquals(13, downloadable.size)
        assertTrue(downloadable.all { !it.cyrillic && !it.japanese })
        newLatin.forEach { lang ->
            val pack = catalog.first { it.code == lang.code }
            assertFalse(pack.bundled)
            assertTrue(lang.isLatin)
            assertNotNull(pack.nativeName)
        }
        assertTrue(StripLang.RU.isCyrillic && StripLang.UK.isCyrillic)
        assertFalse(StripLang.JA.isLatin)
    }

    @Test
    fun nextWordPredictionCoversMajorLatinLanguagesAndFallsBackCleanly() {
        val predictor = NextWordPredictor()
        assertEquals("cómo", predictor.predict("hola", StripLang.ES).first())
        assertEquals("días", predictor.predict("Buenos", StripLang.ES).first())
        assertEquals("Días", predictor.predict("buenos", StripLang.ES, capitalize = true).first())
        assertEquals("tal", predictor.predict("qué", StripLang.ES).first())
        assertEquals("Morgen", predictor.predict("Guten", StripLang.DE).first())
        assertEquals("beaucoup", predictor.predict("merci", StripLang.FR).first())
        assertEquals("mille", predictor.predict("grazie", StripLang.IT).first())
        assertEquals("dia", predictor.predict("bom", StripLang.PT).first())
        listOf(StripLang.NL, StripLang.TR, StripLang.CS, StripLang.SV, StripLang.FI, StripLang.RO, StripLang.HU, StripLang.PL)
            .forEach { assertTrue(it.name, predictor.predict("hola", it).isEmpty()) }
        assertTrue(predictor.predictedSet("hola", StripLang.ES).contains("como"))
    }
}
