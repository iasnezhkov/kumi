// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.protocol

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class LiveStateTest {

    @Test
    fun plannerGroupsContiguousWordsUpToFourteen() {
        val addresses = (0 until 20).map { 0x1000L + 4L * it } + listOf(0x2000L, 0x2004L, 0x3000L)
        assertEquals(
            listOf(MemoryRun(0x1000L, 14), MemoryRun(0x1038L, 6), MemoryRun(0x2000L, 2), MemoryRun(0x3000L, 1)),
            MemoryPlanner.plan(addresses.shuffled()),
        )
        assertEquals(emptyList<MemoryRun>(), MemoryPlanner.plan(emptyList()))
        assertThrows(ProtocolException::class.java) { MemoryPlanner.plan(listOf(0x1002L)) }
    }

    @Test
    fun liveMapWordsAndRuns() {
        val words = LiveStateMap.wordAddresses(0)
        assertEquals(16, words.size)
        assertEquals(
            listOf(
                MemoryRun(0x0011288CL, 2),
                MemoryRun(0x00113A4CL, 2),
                MemoryRun(0x001140BCL, 4),
                MemoryRun(0x0011415CL, 4),
                MemoryRun(0x001141FCL, 4),
            ),
            MemoryPlanner.plan(words),
        )
        assertEquals(0x001140BCL + 16L * 3 + 2L * 5, LiveStateMap.tableAddress(LiveStateMap.EQ_FREQUENCY, 3, 5))
        assertThrows(ProtocolException::class.java) { LiveStateMap.tableAddress(LiveStateMap.EQ_Q, 10, 0) }
    }

    @Test
    fun wordImageExtractsLittleEndianBytes() {
        val image = WordImage(mapOf(0x112890L to 0x3C000000L, 0x112894L to 0x00000002L, 0x113A4CL to 0xFF000100L))
        assertEquals(60, image.u8(0x112893L))
        assertEquals(0x023C, image.u16(0x112893L))
        assertEquals(256, image.s16(0x113A4CL))
        assertEquals(-256, image.s16(0x113A4EL))
        assertThrows(ProtocolException::class.java) { image.u8(0x200000L) }
    }

    @Test
    fun demoLiveStateMatchesDumpValues() {
        val demo = DemoTransport()
        val state = SnapCase(demo).readLiveState(4)
        assertEquals(60, state.volumeRaw)
        assertEquals(1, state.volumeFlag)
        assertEquals(0, state.getter02Shadow)
        assertEquals(0, state.getter03Shadow)
        assertEquals(1, state.recordMarker)
        assertEquals(listOf(32, 64, 125, 250, 500, 1000, 2000, 4000), state.frequencies)
        assertEquals(List(8) { 182 }, state.qRaw)
        assertEquals(DemoTransport.DEFAULT_GAINS[4], state.gainRaw)
        assertFalse(state.looksLikeEcho)
        assertEquals(5, demo.requests.size)
        assertTrue(demo.requests.all { it[2].toInt() == 0 && it[3].toInt() in 8..60 })
    }

    @Test
    fun zeroTablesAreFlaggedAsPossibleEcho() {
        val echo = FakeTransport { it.copyOf() }
        assertTrue(SnapCase(echo).readLiveState(0).looksLikeEcho)
    }
}
