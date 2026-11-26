// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class KeyActionTest {
    @Test
    fun everyKeyThePickerOffersBecomesAnActionTheFirmwareAccepts() {
        HidUsages.selectable.forEach { (usage, name) ->
            assertTrue("$name", HidUsages.keyAction(usage).valid)
            assertTrue("Ctrl+$name", HidUsages.chord(usage, 0x01).valid)
        }
        HidUsages.consumer.keys.forEach { assertTrue("media 0x%x".format(it), KeyAction.Consumer(it).valid) }
    }

    @Test
    fun modifierKeysAreSentAsModifiers() {
        assertEquals(KeyAction.Modifiers(0x01), HidUsages.keyAction(0xE0))
        assertEquals(KeyAction.Modifiers(0x80), HidUsages.keyAction(0xE7))
        assertEquals(KeyAction.Modifiers(0x03), HidUsages.chord(0xE1, 0x01))
        assertEquals(KeyAction.KeyWithModifiers(0x06, 0x01), HidUsages.chord(0x06, 0x01))
        assertEquals(KeyAction.Key(0x29), HidUsages.chord(0x29, 0))
        assertFalse(KeyAction.Key(0xE0).valid)
        assertFalse(KeyAction.Key(0x03).valid)
    }

    @Test
    fun layerSwitchesNeverTargetTheBaseLayer() {
        assertEquals(listOf(1, 2, 3), HidUsages.layerTargets(0, KeymapEntry.LAYERS))
        assertEquals(listOf(1, 3), HidUsages.layerTargets(2, KeymapEntry.LAYERS))
        assertEquals(emptyList<Int>(), HidUsages.layerTargets(1, 2))
        assertFalse(KeyAction.LayerHold(0).valid)
        assertFalse(KeyAction.LayerToggle(KeymapEntry.LAYERS).valid)
        assertTrue(KeyAction.LayerToggle(3).valid)
        assertFalse(KeyAction.Unsupported("\"Sticky\"").valid)
    }

    @Test
    fun qmkTapHoldOneShotMacroAndCaseHotkeysValidate() {
        assertTrue(KeyAction.TapHoldMod(0x29, 0x01).valid)
        assertFalse(KeyAction.TapHoldMod(0x29, 0).valid)
        assertFalse(KeyAction.TapHoldMod(0xE0, 0x01).valid)
        assertTrue(KeyAction.TapHoldLayer(0x2C, 1).valid)
        assertFalse(KeyAction.TapHoldLayer(0x2C, 0).valid)
        assertTrue(KeyAction.OneShotMod(0x02).valid)
        assertFalse(KeyAction.OneShotMod(0).valid)
        assertTrue(KeyAction.Macro(0).valid)
        assertTrue(KeyAction.Macro(KumiLimits.MACRO_SLOTS - 1).valid)
        assertFalse(KeyAction.Macro(KumiLimits.MACRO_SLOTS).valid)
        CaseHotkey.entries.forEach { assertTrue(KeyAction.CaseControl(it).valid) }
        MouseAction.entries.forEach { assertTrue(KeyAction.Mouse(it).valid) }
        assertTrue(KeyAction.CapsWord.valid)
        assertTrue(KeyCombo(0x0D, 0x0E, KeyAction.Key(0x29)).valid)
        assertTrue(KeyCombo(0xE1, 0xE5, KeyAction.CapsWord).valid)
        assertFalse(KeyCombo(0x0D, 0x0D, KeyAction.Key(0x29)).valid)
        assertFalse(KeyCombo(0x03, 0x0E, KeyAction.Key(0x29)).valid)
        assertFalse(KeyCombo(0xE8, 0x0E, KeyAction.Key(0x29)).valid)
        assertFalse(KeyCombo(0x0D, 0x0E, KeyAction.Transparent).valid)
        assertTrue(KeyConfig(200, 3_000, 50).valid)
        assertFalse(KeyConfig(50, 3_000, 50).valid)
        assertFalse(KeyConfig(200, 3_000, 5).valid)
        assertTrue(MacroProgram(listOf(MacroStep(0x04, 0x02))).valid)
    }
}
