// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.view.KeyEvent
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.mindone.snapcase.data.AppSettings
import org.mindone.snapcase.ime.SnapStripImeService

class CaseKeyNavTest {
    @Test
    fun enterAndTabCycleTabsForwardAndBackward() {
        val bus = ScreenKeyBus()
        var page = 0
        bus.tabHandler = { reverse ->
            page = if (reverse) (page + 2) % 3 else (page + 1) % 3
            true
        }
        bus.tabDirectHandler = { idx ->
            if (idx in 0 until 3) {
                page = idx
                true
            } else {
                false
            }
        }

        assertTrue(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER, repeat = false, shift = false, otherMods = false, textFocused = false))
        assertEquals(1, page)
        assertTrue(bus.dispatchKey(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_ENTER, repeat = false, shift = false, otherMods = false, textFocused = false))

        assertTrue(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER, repeat = false, shift = true, otherMods = false, textFocused = false))
        assertEquals(0, page)

        assertTrue(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_TAB, repeat = false, shift = false, otherMods = false, textFocused = false))
        assertEquals(1, page)

        assertTrue(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_RIGHT, repeat = false, shift = false, otherMods = false, textFocused = false))
        assertEquals(2, page)

        assertTrue(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_LEFT, repeat = false, shift = false, otherMods = false, textFocused = false))
        assertEquals(1, page)

        assertTrue(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_1, repeat = false, shift = false, otherMods = false, textFocused = false))
        assertEquals(0, page)

        assertTrue(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_3, repeat = false, shift = false, otherMods = false, textFocused = false))
        assertEquals(2, page)
    }

    @Test
    fun gAndBScrollWithRepeatAndYieldToScreenHandler() {
        val bus = ScreenKeyBus()
        val deltas = mutableListOf<Int>()
        bus.scrollHandler = { d ->
            deltas.add(d)
            true
        }

        assertTrue(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_B, repeat = false, shift = false, otherMods = false, textFocused = false))
        assertTrue(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_B, repeat = true, shift = false, otherMods = false, textFocused = false))
        assertTrue(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_G, repeat = false, shift = false, otherMods = false, textFocused = false))
        assertTrue(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_PAGE_DOWN, repeat = false, shift = false, otherMods = false, textFocused = false))
        assertTrue(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_PAGE_UP, repeat = false, shift = false, otherMods = false, textFocused = false))
        assertEquals(listOf(1, 1, -1, 3, -3), deltas)

        var screenHandledG = false
        bus.screenHandler = { code, _ ->
            if (code == KeyEvent.KEYCODE_G) {
                screenHandledG = true
                true
            } else {
                false
            }
        }
        assertTrue(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_G, repeat = false, shift = false, otherMods = false, textFocused = false))
        assertTrue(screenHandledG)
        assertEquals(5, deltas.size)
    }

    @Test
    fun backspaceTriggersGlobalBackAndRespectsSuppress() {
        val bus = ScreenKeyBus()
        var backed = 0
        bus.globalBackHandler = {
            backed++
            true
        }

        assertTrue(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DEL, repeat = false, shift = false, otherMods = false, textFocused = false))
        assertEquals(1, backed)
        assertTrue(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ESCAPE, repeat = false, shift = false, otherMods = false, textFocused = false))
        assertEquals(2, backed)

        bus.pushSuppress()
        assertFalse(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DEL, repeat = false, shift = false, otherMods = false, textFocused = false))
        assertEquals(2, backed)
        bus.popSuppress()

        assertFalse(bus.dispatchKey(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DEL, repeat = false, shift = false, otherMods = false, textFocused = true))
        assertEquals(2, backed)
    }

    @Test
    fun defaultSettingsEnableBackspaceBackAndMediaDoubleTap() {
        val defaults = AppSettings()
        assertTrue(defaults.backspaceAsBack)
        assertTrue(defaults.mediaDoubleTap)
        assertTrue(defaults.homeTypeToSearch)
        assertEquals(20, defaults.imeHapticDurationMs)
    }

    @Test
    fun imeContentInsetsRespectWrapContentAndFullScreenDecor() {
        assertEquals(0, SnapStripImeService.computeImeContentTopInsets(decorHeight = 202, rootHeight = 202, displayHeight = 1240, shown = true))
        assertEquals(0, SnapStripImeService.computeImeContentTopInsets(decorHeight = 492, rootHeight = 492, displayHeight = 1240, shown = true))
        assertEquals(971, SnapStripImeService.computeImeContentTopInsets(decorHeight = 1173, rootHeight = 202, displayHeight = 1240, shown = true))
        assertEquals(496, SnapStripImeService.computeImeContentTopInsets(decorHeight = 1240, rootHeight = 1240, displayHeight = 1240, shown = true))
        assertEquals(202, SnapStripImeService.computeImeContentTopInsets(decorHeight = 202, rootHeight = 202, displayHeight = 1240, shown = false))
    }

    @Test
    fun mnemonicAllocatorPrefersFirstLetterAndRespectsGlobalReservedKeys() {
        val map = allocateMnemonicKeys(
            specs = listOf(
                ShortcutSpec("measure", "Check headphones", KeyEvent.KEYCODE_C),
                ShortcutSpec("left", "Left", KeyEvent.KEYCODE_L),
                ShortcutSpec("right", "Right", KeyEvent.KEYCODE_R),
                ShortcutSpec("both", "Both", KeyEvent.KEYCODE_O),
                ShortcutSpec("phase", "Phase", KeyEvent.KEYCODE_P),
                ShortcutSpec("group", "Group", KeyEvent.KEYCODE_Z),
            ),
            locale = Locale.ENGLISH,
        )
        assertEquals("C", map.label("measure"))
        assertEquals("L", map.label("left"))
        assertEquals("R", map.label("right"))
        assertEquals("O", map.label("both"))
        assertEquals("P", map.label("phase"))
        assertFalse(map.code("both") in GLOBAL_RESERVED_KEYS)
        assertFalse(map.code("group") in GLOBAL_RESERVED_KEYS)
        assertEquals(map.bindings.size, map.bindings.map { it.keyCode }.toSet().size)
    }

    @Test
    fun mnemonicAllocatorAdaptsToRussianGermanFrenchPolishAndCjk() {
        val ru = allocateMnemonicKeys(
            specs = listOf(
                ShortcutSpec("sound", "\u0417\u0432\u0443\u043A", KeyEvent.KEYCODE_S),
                ShortcutSpec("eq", "\u042D\u043A\u0432\u0430\u043B\u0430\u0439\u0437\u0435\u0440", KeyEvent.KEYCODE_E),
                ShortcutSpec("keys", "\u041A\u043B\u0430\u0432\u0438\u0448\u0438", KeyEvent.KEYCODE_K),
                ShortcutSpec("xfeed", "\u041E\u0431\u044A\u0451\u043C\u043D\u044B\u0439", KeyEvent.KEYCODE_X),
                ShortcutSpec("awake", "\u041D\u0435 \u0437\u0430\u0441\u044B\u043F\u0430\u0442\u044C", KeyEvent.KEYCODE_A),
                ShortcutSpec("strip", "\u041F\u043E\u043B\u043E\u0441\u0430", KeyEvent.KEYCODE_T),
                ShortcutSpec("shortcuts", "\u0421\u043E\u0447\u0435\u0442\u0430\u043D\u0438\u044F", KeyEvent.KEYCODE_H),
            ),
            locale = Locale.forLanguageTag("ru"),
        )
        assertEquals("Z", ru.label("sound"))
        assertEquals("E", ru.label("eq"))
        assertEquals("K", ru.label("keys"))
        assertEquals("O", ru.label("xfeed"))
        assertEquals("N", ru.label("awake"))
        assertEquals("P", ru.label("strip"))
        assertEquals("S", ru.label("shortcuts"))

        val fr = allocateMnemonicKeys(
            specs = listOf(
                ShortcutSpec("eq", "\u00C9galiseur", KeyEvent.KEYCODE_E),
                ShortcutSpec("left", "Gauche", KeyEvent.KEYCODE_L),
                ShortcutSpec("right", "Droite", KeyEvent.KEYCODE_R),
            ),
            locale = Locale.FRENCH,
        )
        assertEquals("E", fr.label("eq"))
        assertEquals("A", fr.label("left"))
        assertEquals("D", fr.label("right"))
        assertFalse(fr.code("left") in GLOBAL_RESERVED_KEYS)

        val pl = allocateMnemonicKeys(
            specs = listOf(
                ShortcutSpec("save", "Zapisz", KeyEvent.KEYCODE_S),
                ShortcutSpec("group", "Grupa", KeyEvent.KEYCODE_Z, fixedPreference = listOf(KeyEvent.KEYCODE_Z, KeyEvent.KEYCODE_X)),
            ),
            locale = Locale.forLanguageTag("pl"),
        )
        assertEquals("Z", pl.label("save"))
        assertEquals("X", pl.label("group"))

        val ja = allocateMnemonicKeys(
            specs = listOf(
                ShortcutSpec("sound", "\u30B5\u30A6\u30F3\u30C9", KeyEvent.KEYCODE_S),
                ShortcutSpec("eq", "\u30A4\u30B3\u30E9\u30A4\u30B6\u30FC", KeyEvent.KEYCODE_E),
                ShortcutSpec("keys", "\u30AD\u30FC", KeyEvent.KEYCODE_K),
                ShortcutSpec("case", "\u30B1\u30FC\u30B9", KeyEvent.KEYCODE_C),
            ),
            locale = Locale.JAPANESE,
        )
        assertEquals("S", ja.label("sound"))
        assertEquals("E", ja.label("eq"))
        assertEquals("K", ja.label("keys"))
        assertEquals("C", ja.label("case"))
    }

    @Test
    fun eqPresetsStandardsAndGenresHaveNoDuplicatesOrReservedKeys() {
        val standards = allocateMnemonicKeys(
            specs = listOf(
                ShortcutSpec("save", "Save", KeyEvent.KEYCODE_S),
                ShortcutSpec("group", "Group", KeyEvent.KEYCODE_Z, fixedPreference = listOf(KeyEvent.KEYCODE_Z, KeyEvent.KEYCODE_X)),
                ShortcutSpec("flat", "Flat", KeyEvent.KEYCODE_F),
                ShortcutSpec("harman_oe", "Harman Over-Ear", KeyEvent.KEYCODE_H),
                ShortcutSpec("harman", "Harman In-Ear", KeyEvent.KEYCODE_I),
                ShortcutSpec("studio", "Studio Neutral", KeyEvent.KEYCODE_N),
                ShortcutSpec("diffuse", "Diffuse Field", KeyEvent.KEYCODE_D),
                ShortcutSpec("warm", "Warm & Smooth", KeyEvent.KEYCODE_W),
                ShortcutSpec("late", "Late Night", KeyEvent.KEYCODE_L),
                ShortcutSpec("vshape", "V-Shape / Bass", KeyEvent.KEYCODE_V),
            ),
            locale = Locale.ENGLISH,
        )
        assertEquals("S", standards.label("save"))
        assertEquals("Z", standards.label("group"))
        assertEquals("F", standards.label("flat"))
        assertEquals("H", standards.label("harman_oe"))
        assertEquals("I", standards.label("harman"))
        assertEquals("N", standards.label("studio"))
        assertEquals("D", standards.label("diffuse"))
        assertEquals("W", standards.label("warm"))
        assertEquals("L", standards.label("late"))
        assertEquals("V", standards.label("vshape"))
        assertTrue(standards.bindings.none { it.keyCode in GLOBAL_RESERVED_KEYS })

        val genres = allocateMnemonicKeys(
            specs = listOf(
                ShortcutSpec("save", "Save", KeyEvent.KEYCODE_S),
                ShortcutSpec("group", "Group", KeyEvent.KEYCODE_Z, fixedPreference = listOf(KeyEvent.KEYCODE_Z, KeyEvent.KEYCODE_X)),
                ShortcutSpec("edm", "EDM & Electronic", KeyEvent.KEYCODE_E),
                ShortcutSpec("rock", "Rock & Metal", KeyEvent.KEYCODE_R),
                ShortcutSpec("hiphop", "Hip-Hop & R&B", KeyEvent.KEYCODE_H),
                ShortcutSpec("jazz", "Jazz & Blues", KeyEvent.KEYCODE_J),
                ShortcutSpec("classical", "Classical", KeyEvent.KEYCODE_C),
                ShortcutSpec("pop", "Pop", KeyEvent.KEYCODE_P),
                ShortcutSpec("acoustic", "Acoustic & Indie", KeyEvent.KEYCODE_A),
                ShortcutSpec("voice", "Podcasts & Voice", KeyEvent.KEYCODE_V),
            ),
            locale = Locale.ENGLISH,
        )
        assertEquals("S", genres.label("save"))
        assertEquals("Z", genres.label("group"))
        assertEquals("E", genres.label("edm"))
        assertEquals("R", genres.label("rock"))
        assertEquals("H", genres.label("hiphop"))
        assertEquals("J", genres.label("jazz"))
        assertEquals("C", genres.label("classical"))
        assertEquals("P", genres.label("pop"))
        assertEquals("A", genres.label("acoustic"))
        assertEquals("V", genres.label("voice"))
        assertTrue(genres.bindings.none { it.keyCode in GLOBAL_RESERVED_KEYS })
    }
}
