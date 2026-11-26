// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.remap

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class RemapTableTest {
    private val keyA = 29
    private val keyB = 30
    private val keyC = 31
    private val ctrlMeta = Keys.META_CTRL_ON or Keys.META_CTRL_LEFT_ON
    private val altMeta = Keys.META_ALT_ON or Keys.META_ALT_LEFT_ON
    private val metaMeta = Keys.META_META_ON or Keys.META_META_LEFT_ON

    @Test
    fun parseAndSerializeRoundTrip() {
        val table = RemapTable.parse(" 115:113 , 29:30,57:117")
        assertEquals(mapOf(115 to 113, 29 to 30, 57 to 117), table.entries)
        assertEquals("29:30,57:117,115:113", table.serialize())
        assertEquals(table, RemapTable.parse(table.serialize()))
        assertEquals(RemapTable.EMPTY, RemapTable.parse(""))
        assertEquals(RemapTable.EMPTY, RemapTable.parse(null))
    }

    @Test
    fun invalidTablesAreRejected() {
        for (text in listOf("29", "29:30:31", "a:b", "29:29", "0:30", "29:-1", "29:401", "29:30,29:31")) {
            assertThrows(IllegalArgumentException::class.java) { RemapTable.parse(text) }
        }
        assertEquals(RemapTable.EMPTY, RemapTable.parseOrEmpty("garbage"))
        assertThrows(IllegalArgumentException::class.java) { RemapTable.EMPTY.with(29, 29) }
    }

    @Test
    fun withAndWithout() {
        val table = RemapTable.of(keyA to keyB).with(keyA, keyC).with(keyB, Keys.UNKNOWN)
        assertEquals(mapOf(keyA to keyC, keyB to 0), table.entries)
        assertEquals(mapOf(keyB to 0), table.without(keyA).entries)
    }

    @Test
    fun unmappedKeysPassThrough() {
        val engine = RemapEngine(RemapTable.of(keyA to keyB))
        assertEquals(RemapDecision.PassThrough, engine.process(KeyInput(keyC, true, 0)))
        assertEquals(RemapDecision.PassThrough, engine.process(KeyInput(keyC, false, 0)))
    }

    @Test
    fun simpleMapReplacesDownAndUp() {
        val engine = RemapEngine(RemapTable.of(keyA to keyB))
        assertEquals(
            RemapDecision.Replace(listOf(KeyOutput(keyB, true, Keys.META_SHIFT_ON, 0))),
            engine.process(KeyInput(keyA, true, Keys.META_SHIFT_ON)),
        )
        assertEquals(
            RemapDecision.Replace(listOf(KeyOutput(keyB, true, 0, 1))),
            engine.process(KeyInput(keyA, true, 0, repeatCount = 1)),
        )
        assertEquals(RemapDecision.Replace(listOf(KeyOutput(keyB, false, 0, 0))), engine.process(KeyInput(keyA, false, 0)))
    }

    @Test
    fun disabledKeyIsSwallowed() {
        val engine = RemapEngine(RemapTable.of(keyA to Keys.UNKNOWN))
        assertEquals(RemapDecision.Replace(emptyList()), engine.process(KeyInput(keyA, true, 0)))
        assertEquals(RemapDecision.Replace(emptyList()), engine.process(KeyInput(keyA, false, 0)))
    }

    @Test
    fun remappedModifierAppliesToFollowingKeys() {
        val engine = RemapEngine(RemapTable.of(Keys.CAPS_LOCK to Keys.CTRL_LEFT))
        assertEquals(
            RemapDecision.Replace(listOf(KeyOutput(Keys.CTRL_LEFT, true, ctrlMeta, 0))),
            engine.process(KeyInput(Keys.CAPS_LOCK, true, 0)),
        )
        assertEquals(RemapDecision.Replace(listOf(KeyOutput(keyC, true, ctrlMeta, 0))), engine.process(KeyInput(keyC, true, 0)))
        assertEquals(RemapDecision.Replace(listOf(KeyOutput(keyC, false, ctrlMeta, 0))), engine.process(KeyInput(keyC, false, 0)))
        assertEquals(
            RemapDecision.Replace(listOf(KeyOutput(Keys.CTRL_LEFT, false, 0, 0))),
            engine.process(KeyInput(Keys.CAPS_LOCK, false, 0)),
        )
        assertEquals(0, engine.virtualMeta)
        assertEquals(RemapDecision.PassThrough, engine.process(KeyInput(keyC, true, 0)))
    }

    @Test
    fun swappedModifierStripsPhysicalMeta() {
        val engine = RemapEngine(RemapTable.of(Keys.ALT_LEFT to Keys.META_LEFT))
        assertEquals(
            RemapDecision.Replace(listOf(KeyOutput(Keys.META_LEFT, true, metaMeta, 0))),
            engine.process(KeyInput(Keys.ALT_LEFT, true, altMeta)),
        )
        assertEquals(
            RemapDecision.Replace(listOf(KeyOutput(keyC, true, metaMeta or Keys.META_SHIFT_ON, 0))),
            engine.process(KeyInput(keyC, true, altMeta or Keys.META_SHIFT_ON)),
        )
        assertEquals(
            RemapDecision.Replace(listOf(KeyOutput(Keys.META_LEFT, false, 0, 0))),
            engine.process(KeyInput(Keys.ALT_LEFT, false, 0)),
        )
    }

    @Test
    fun otherDevicesAreIgnored() {
        val engine = RemapEngine(RemapTable.of(keyA to keyB))
        assertEquals(RemapDecision.PassThrough, engine.process(KeyInput(keyA, true, 0, fromTargetDevice = false)))
    }

    @Test
    fun releaseFollowsTheMappingActiveAtPress() {
        val engine = RemapEngine(RemapTable.of(keyA to keyB))
        engine.process(KeyInput(keyA, true, 0))
        engine.table = RemapTable.EMPTY
        assertEquals(RemapDecision.Replace(listOf(KeyOutput(keyB, false, 0, 0))), engine.process(KeyInput(keyA, false, 0)))
        assertEquals(RemapDecision.PassThrough, engine.process(KeyInput(keyA, true, 0)))
    }

    @Test
    fun resetClearsHeldModifiers() {
        val engine = RemapEngine(RemapTable.of(Keys.CAPS_LOCK to Keys.CTRL_LEFT))
        engine.process(KeyInput(Keys.CAPS_LOCK, true, 0))
        engine.reset()
        assertEquals(RemapDecision.PassThrough, engine.process(KeyInput(keyC, true, 0)))
    }
}
