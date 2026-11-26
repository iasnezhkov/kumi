// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.protocol

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class StockControlsTest {

    @Test
    fun demoAudioStateMatchesDefaults() {
        val state = SnapCase(DemoTransport()).readAudioState()
        assertEquals(2, state.usbClass)
        assertEquals(48000L, state.sampleRateHz)
        assertEquals(DacFilter.FAST_LOW_LATENCY, state.dacFilter)
        assertFalse(state.eqEnabled)
    }

    @Test
    fun dacFilterWriteReadsBackAndKeepsNeighbours() {
        val demo = DemoTransport()
        val case = SnapCase(demo) { true }
        val outcome = case.writeDacFilter(DacFilter.SLOW_LOW_LATENCY)
        assertTrue(outcome.acknowledged)
        assertEquals(true, outcome.readBackMatches)
        assertEquals(DacFilter.SLOW_LOW_LATENCY, case.readAudioState().dacFilter)
        val word = case.readMemory(StockState.DAC_FILTER and 3L.inv())[0]
        assertEquals(0x00000200L, word)
    }

    @Test
    fun disabledWritesSendNothing() {
        val demo = DemoTransport()
        assertThrows(WritesDisabledException::class.java) { SnapCase(demo) { false }.writeDacFilter(DacFilter.NON_OVERSAMPLING) }
        assertTrue(demo.requests.isEmpty())
    }

    @Test
    fun onlyAllowlistedAddressesAreWritable() {
        val demo = DemoTransport()
        val case = SnapCase(demo) { true }
        assertThrows(ProtocolException::class.java) { case.writeRamByte(LiveStateMap.VOLUME, 10) }
        assertThrows(ProtocolException::class.java) { case.writeRamByte(StockState.EQ_PROFILE, 1) }
        assertTrue(demo.requests.isEmpty())
    }
}
