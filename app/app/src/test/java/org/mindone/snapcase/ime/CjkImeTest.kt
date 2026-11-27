// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.mindone.snapcase.ime.dict.PinyinDict
import org.mindone.snapcase.ime.dict.PinyinEntry
import org.mindone.snapcase.ime.dict.SnapDictBinary
import org.mindone.snapcase.ime.dict.SuggestionEngine

class CjkImeTest {
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

    @Test
    fun hangulComposerAssemblesSyllablesCompoundVowelsFinalsAndBackspace() {
        assertEquals("한글", HangulComposer.composeFromLatin("gksrmf"))
        assertEquals("안녕하세요", HangulComposer.composeFromLatin("dkssudgktpdy"))
        assertEquals("대한민국", HangulComposer.composeFromLatin("eogksalsrnr"))
        assertEquals("과일", HangulComposer.composeFromLatin("rhkdlf"))
        assertEquals("달기", HangulComposer.composeFromLatin("ekfrl"))
        assertEquals("닭", HangulComposer.composeFromLatin("ekfr"))
        assertEquals("빨리", HangulComposer.composeFromLatin("Qkffl"))
        assertEquals("예쁜", HangulComposer.composeFromLatin("dPQms"))

        val composer = HangulComposer()
        "ekfr".forEach { assertTrue(composer.feedLatin(it)) }
        assertEquals("닭", composer.text())
        assertTrue(composer.backspace())
        assertEquals("달", composer.text())
        assertTrue(composer.backspace())
        assertEquals("다", composer.text())
        assertTrue(composer.backspace())
        assertEquals("ㄷ", composer.text())
        assertTrue(composer.backspace())
        assertEquals("", composer.text())
        assertFalse(composer.backspace())

        val vowelComp = HangulComposer()
        "rhk".forEach { vowelComp.feedLatin(it) }
        assertEquals("과", vowelComp.text())
        assertTrue(vowelComp.backspace())
        assertEquals("고", vowelComp.text())
        assertTrue(vowelComp.backspace())
        assertEquals("ㄱ", vowelComp.text())
    }

    @Test
    fun bundledKoreanDictionaryAndImeProcessorWork() {
        val koFile = File("src/main/assets/dict/ko.snapdict")
        assertTrue("missing bundled Korean dictionary ${koFile.absolutePath}", koFile.isFile)
        val koDict = koFile.inputStream().use(SnapDictBinary::load)
        assertTrue(koDict.count >= 10_000)

        val engine = SuggestionEngine()
        val res = engine.suggest("안녕", "ko", koDict)
        assertTrue("expected 안녕하세요 in ${res.suggestions}", res.suggestions.any { it.startsWith("안녕") })

        val processor = ImeKeyProcessor(
            engine = engine,
            dictProvider = { lang -> if (lang == StripLang.KO) koDict else null },
        )
        val target = FakeTarget()
        processor.startInput(passwordField = false, textBefore = "")
        processor.setLanguage(StripLang.KO)

        "dkssud".forEach { processor.onCharTyped(it, physicalShifted = false, target, autoDetectLang = false) }
        assertEquals("안녕", target.activeComposing)
        val snap = processor.snapshot(false, 0, false, true, emptyList())
        assertTrue("expected Korean suggestions, got ${snap.suggestions}", snap.suggestions.isNotEmpty())

        assertTrue(processor.onBackspace(target))
        assertEquals("안녀", target.activeComposing)

        processor.onCharTyped('d', physicalShifted = false, target, autoDetectLang = false)
        assertEquals("안녕", target.activeComposing)

        processor.onCharTyped(' ', physicalShifted = false, target, autoDetectLang = false)
        assertEquals("안녕 ", target.committed.toString())
        assertEquals("", target.activeComposing)
    }

    @Test
    fun pinyinSegmentationAndBinaryRoundTripWork() {
        assertEquals(listOf("ni", "hao"), PinyinEngine.segment("nihao"))
        assertEquals(listOf("zhong", "guo"), PinyinEngine.segment("zhongguo"))
        assertEquals(listOf("xi", "an"), PinyinEngine.segment("xi'an"))
        assertEquals(listOf("fang", "an"), PinyinEngine.segment("fangan"))
        assertEquals(listOf("tian", "an", "men"), PinyinEngine.segment("tiananmen"))
        assertEquals("ni'hao", PinyinEngine.formatComposing("nihao"))

        val packed = PinyinDict.pack(
            listOf(
                PinyinEntry("nihao", "你好", 250),
                PinyinEntry("nihao", "拟好", 100),
                PinyinEntry("ni", "你", 240),
                PinyinEntry("hao", "好", 230),
            ),
        )
        val dict = PinyinDict.fromBytes(packed)
        assertEquals(4, dict.count)
        assertEquals(listOf("你好", "拟好"), dict.lookupExact("nihao").map { it.first })

        val yaml = """
            ---
            name: test
            ...
            你好	ni hao	95%
            中国	zhong guo	80%
            旅	lv	50%
        """.trimIndent()
        val compiled = PinyinDict.fromBytes(PinyinDict.compileFromRimeYaml(yaml))
        assertEquals("你好", compiled.lookupExact("nihao").firstOrNull()?.first)
        assertEquals("中国", compiled.lookupExact("zhongguo").firstOrNull()?.first)
        assertEquals("旅", compiled.lookupExact("lv").firstOrNull()?.first)
    }

    @Test
    fun bundledSimplifiedAndTraditionalPinyinDictionariesAndImeProcessorWork() {
        val cnFile = File("src/main/assets/dict/zh_cn.snappy")
        val twFile = File("src/main/assets/dict/zh_tw.snappy")
        assertTrue("missing bundled zh_cn dictionary ${cnFile.absolutePath}", cnFile.isFile)
        assertTrue("missing bundled zh_tw dictionary ${twFile.absolutePath}", twFile.isFile)
        val cnDict = cnFile.inputStream().use(PinyinDict::load)
        val twDict = twFile.inputStream().use(PinyinDict::load)
        assertTrue(cnDict.count >= 15_000)
        assertTrue(twDict.count >= 15_000)

        val cnHits = PinyinEngine.lookup("nihao", cnDict)
        assertTrue("expected 你好 in $cnHits", cnHits.contains("你好"))
        val cnCountry = PinyinEngine.lookup("zhongguo", cnDict)
        assertTrue("expected 中国 in $cnCountry", cnCountry.contains("中国"))
        val twCountry = PinyinEngine.lookup("zhongguo", twDict)
        assertTrue("expected 中國 in $twCountry", twCountry.contains("中國"))

        val processor = ImeKeyProcessor(
            engine = SuggestionEngine(),
            dictProvider = { null },
            pinyinDictProvider = { lang -> if (lang == StripLang.ZH_TW) twDict else cnDict },
        )
        val target = FakeTarget()
        processor.startInput(passwordField = false, textBefore = "")
        processor.setLanguage(StripLang.ZH_CN)

        "nihao".forEach { processor.onCharTyped(it, physicalShifted = false, target, autoDetectLang = false) }
        assertEquals("ni'hao", target.activeComposing)
        val snap = processor.snapshot(false, 0, false, true, emptyList())
        assertTrue("expected 你好 in ${snap.suggestions}", snap.suggestions.contains("你好"))

        processor.onCharTyped(' ', physicalShifted = false, target, autoDetectLang = false)
        assertEquals("你好", target.committed.toString())
        assertEquals("", target.activeComposing)

        assertTrue(processor.onBackspace(target))
        assertEquals("", target.committed.toString())
        assertEquals("ni'hao", target.activeComposing)

        assertTrue(processor.onEnter(target))
        assertEquals("nihao", target.committed.toString())

        target.committed.clear()
        processor.setLanguage(StripLang.ZH_TW)
        "zhongguo".forEach { processor.onCharTyped(it, physicalShifted = false, target, autoDetectLang = false) }
        val twSnap = processor.snapshot(false, 0, false, true, emptyList())
        assertTrue("expected 中國 in ${twSnap.suggestions}", twSnap.suggestions.contains("中國"))
        processor.onCharTyped(' ', physicalShifted = false, target, autoDetectLang = false)
        assertTrue(target.committed.toString().contains("中國"))
    }
}
