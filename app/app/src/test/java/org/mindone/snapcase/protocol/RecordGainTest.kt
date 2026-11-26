// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.protocol

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class RecordGainTest {

    @Test
    fun encodesVendorLayout() {
        val frame = SnapProtocol.encodeRecordGain(SnapProtocol.recordGainRaw(-6.5))
        assertEquals("4b010202" + "80f9", frame.toHex(6))
        assertEquals(SnapProtocol.REPORT_SIZE, frame.size)
    }

    @Test
    fun quantizesLikeTheFirmware() {
        assertEquals(0, SnapProtocol.quantizeRecordGain(0))
        assertEquals(384, SnapProtocol.quantizeRecordGain(200))
        assertEquals(0, SnapProtocol.quantizeRecordGain(191))
        assertEquals(-384, SnapProtocol.quantizeRecordGain(-192))
        assertEquals(0, SnapProtocol.quantizeRecordGain(-191))
        assertEquals(3072, SnapProtocol.quantizeRecordGain(3072))
        assertEquals(-6144, SnapProtocol.quantizeRecordGain(-6144))
        assertEquals(-1536, SnapProtocol.quantizeRecordGain(SnapProtocol.recordGainRaw(-6.5)))
    }

    @Test
    fun rejectsOutOfRange() {
        assertThrows(ProtocolException::class.java) { SnapProtocol.recordGainRaw(-24.5) }
        assertThrows(ProtocolException::class.java) { SnapProtocol.recordGainRaw(12.5) }
        assertThrows(ProtocolException::class.java) { SnapProtocol.recordGainRaw(Double.NaN) }
    }

    @Test
    fun writeProbesSetsAndReadsBack() {
        val demo = DemoTransport()
        val case = SnapCase(demo) { true }
        val outcome = case.writeRecordGain(4.0)
        assertTrue(outcome.acknowledged)
        assertEquals(true, outcome.readBackMatches)
        assertEquals(listOf(0x80 to 0x0C, 0x01 to 0x02, 0x80 to 0x02), demo.requests.map { (it[1].toInt() and 0xFF) to it[2].toInt() })
        assertEquals(1152, case.recordGainRaw())
    }

    @Test
    fun disabledWriteSendsNothing() {
        val demo = DemoTransport()
        assertThrows(WritesDisabledException::class.java) { SnapCase(demo) { false }.writeRecordGain(3.0) }
        assertTrue(demo.requests.isEmpty())
    }

    @Test
    fun serviceEntryProbesThenSendsResetWithoutReply() {
        val demo = DemoTransport()
        SnapCase(demo) { false }.enterServiceMode()
        assertTrue(demo.serviceRequested)
        assertEquals("4b80" + "0c", demo.requests[0].toHex(3))
        assertArrayEquals(SnapProtocol.encodeServiceEntry(), demo.requests[1])
        assertEquals("4b01ff00", demo.requests[1].toHex(4))
    }

    @Test
    fun serviceEntryNeedsAnAwakeCase() {
        val demo = DemoTransport().apply { asleep = true }
        assertThrows(CaseAsleepException::class.java) { SnapCase(demo).enterServiceMode() }
        assertFalse(demo.serviceRequested)
    }

    @Test
    fun exchangeStillRefusesReset() {
        assertThrows(ProtocolException::class.java) { SnapCase(DemoTransport()) { true }.exchange(SnapProtocol.encodeServiceEntry()) }
    }

    @Test
    fun audioStateHasCaptureTrimAndStreamFields() {
        val state = SnapCase(DemoTransport()).readAudioState()
        assertEquals(1920, state.captureTrimRaw)
        assertEquals(StreamMode.PCM, state.streamMode)
        assertEquals(UsbAudioProfile.UNKNOWN, state.usbProfile)
        assertEquals(null, state.pcmBits)
    }
}
