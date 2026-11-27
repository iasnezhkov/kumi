// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.view.KeyEvent
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
}
