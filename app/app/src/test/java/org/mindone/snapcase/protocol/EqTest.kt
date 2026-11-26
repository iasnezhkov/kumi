// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.protocol

import kotlin.math.abs
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class EqTest {
    private val captured = DemoTransport.MEASURED_BANK0.map { hexToBytes(it).copyOf(64) }

    @Test
    fun decodesMeasuredBank0() {
        val bands = captured.map(EqCodec::decodeReply)
        assertEquals(listOf(32, 64, 125, 250, 500, 1000, 2000, 4000), bands.map { it.frequencyHz })
        assertEquals(List(8) { 182 }, bands.map { it.qRaw })
        assertEquals(listOf(0, 0, 0, 1024, 1024, 1024, 0, 512), bands.map { it.gainRaw })
        assertEquals(List(8) { 2 }, bands.map { it.filterType })
        assertEquals(List(8) { -6 }, bands.map { it.preamp })
        assertEquals((0 until 8).toList(), bands.map { it.band })
        assertTrue(bands.all { it.bank == 0 })
        assertEquals(listOf(1073741824, -2144578408, 1070840999, 2144578408, -1070840999), bands[0].coefficients)
        assertEquals(4.0, bands[3].gainDb, 0.0)
    }

    @Test
    fun readRequestMatchesCapturedTx() {
        assertArrayEquals(hexToBytes("4b80090400030000").copyOf(64), EqCodec.encodeRead(0, 3))
        assertArrayEquals(hexToBytes("4b80090408070000").copyOf(64), EqCodec.encodeRead(8, 7))
        assertThrows(ProtocolException::class.java) { EqCodec.encodeRead(9, 0) }
        assertThrows(ProtocolException::class.java) { EqCodec.encodeRead(0, 8) }
        assertThrows(ProtocolException::class.java) { EqCodec.encodeRead(-1, 0) }
    }

    @Test
    fun writeFrameMirrorsReplyLayout() {
        for (frame in captured) {
            val band = EqCodec.decodeReply(frame)
            val write = EqCodec.encodeWrite(band)
            assertArrayEquals(hexToBytes("4b010921"), write.copyOfRange(0, 4))
            assertArrayEquals(frame.copyOfRange(4, 37), write.copyOfRange(4, 37))
            assertArrayEquals(ByteArray(27), write.copyOfRange(37, 64))
            val echoed = write.copyOf().also { it[1] = 0x80.toByte() }
            assertEquals(band, EqCodec.decodeReply(echoed))
        }
    }

    @Test
    fun writeValidation() {
        val band = EqCodec.decodeReply(captured[0])
        assertThrows(ProtocolException::class.java) { EqCodec.encodeWrite(band.copy(frequencyHz = 0)) }
        assertThrows(ProtocolException::class.java) { EqCodec.encodeWrite(band.copy(qRaw = -1)) }
        assertEquals(0, EqCodec.encodeWrite(band.copy(qRaw = 0)).u16le(30))
        assertThrows(ProtocolException::class.java) { EqCodec.encodeWrite(band.copy(gainRaw = 40000)) }
        assertThrows(ProtocolException::class.java) { EqCodec.encodeWrite(band.copy(filterType = 8)) }
        assertThrows(ProtocolException::class.java) { EqCodec.encodeWrite(band.copy(preamp = 128)) }
        assertThrows(ProtocolException::class.java) { EqCodec.encodeWrite(band.copy(coefficients = listOf(1, 2))) }
        val negative = EqCodec.encodeWrite(band.copy(gainRaw = -384, preamp = -11))
        assertEquals(-384, negative.s16le(32))
        assertEquals(-11, negative[35].toInt())
    }

    @Test
    fun shortReplyIsRejected() {
        assertThrows(ProtocolException::class.java) { EqCodec.decodeReply(captured[0].copyOf(36)) }
        assertThrows(ProtocolException::class.java) { EqCodec.decodeReply(SnapProtocol.encodeGet(0x03)) }
    }

    @Test
    fun applyUsesVendorPayload() {
        assertArrayEquals(hexToBytes("4b010a040000ffff").copyOf(64), EqCodec.encodeApply())
    }

    @Test
    fun designerReproducesStoredCoefficients() {
        val frequencies = listOf(31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0)
        frequencies.forEachIndexed { index, frequency ->
            val expected = EqCodec.decodeReply(captured[index]).coefficients
            val actual = BiquadDesigner.coefficients(FilterType.PEAK, frequency, 0.75, 0.0, 96_000)
            expected.zip(actual).forEach { (e, a) -> assertTrue("band $index: $expected vs $actual", abs(e - a) <= 1) }
        }
    }

    @Test
    fun designerShapes() {
        val flat = BiquadDesigner.coefficients(FilterType.DISABLED, 1000.0, 0.7, 0.0, 48_000)
        assertEquals(listOf(1 shl 30, 0, 0, 0, 0), flat)
        val boost = BiquadDesigner.coefficients(FilterType.PEAK, 1000.0, 0.7, 6.0, 48_000)
        assertTrue(boost[0] > (1 shl 30))
        val lowPass = BiquadDesigner.coefficients(FilterType.LOW_PASS, 1000.0, 0.7, 0.0, 48_000)
        assertEquals(lowPass[0], lowPass[2])
        for (type in FilterType.entries) BiquadDesigner.coefficients(type, 500.0, 0.75, -3.0, 96_000)
        assertThrows(ProtocolException::class.java) { BiquadDesigner.coefficients(FilterType.HIGH_SHELF, 500.0, 0.75, 3.0, 96_000) }
    }

    @Test
    fun designerRejectsBadInput() {
        assertThrows(ProtocolException::class.java) { BiquadDesigner.coefficients(FilterType.PEAK, 48_000.0, 1.0, 0.0, 96_000) }
        assertThrows(ProtocolException::class.java) { BiquadDesigner.coefficients(FilterType.PEAK, 1000.0, 0.0, 0.0, 96_000) }
        assertThrows(ProtocolException::class.java) { BiquadDesigner.coefficients(FilterType.PEAK, 1000.0, 1.0, 0.0, 0) }
        assertThrows(ProtocolException::class.java) { BiquadDesigner.coefficients(FilterType.PEAK, 20_000.0, 0.1, 30.0, 96_000) }
    }
}
