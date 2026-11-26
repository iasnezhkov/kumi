// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class DevInputTest {
    @Test
    fun wordsAndAddressesParseAsHex() {
        assertEquals(0x2000_0000L, DevInput.address("20000000"))
        assertEquals(0x2000_0000L, DevInput.address("0x20000000"))
        assertNull("peek and poke are word-aligned", DevInput.address("20000002"))
        assertEquals(0xFFFF_FFFFL, DevInput.word("ffffffff"))
        assertNull(DevInput.word("100000000"))
        assertNull(DevInput.word(""))
        assertNull(DevInput.word("xyz"))
        assertEquals("000001a5", DevInput.wordText(0x1A5))
    }

    @Test
    fun i2cFieldsStayInsideTheProtocolLimits() {
        assertEquals(0x33, DevInput.i2cAddress("33"))
        assertNull(DevInput.i2cAddress("80"))
        assertEquals(listOf(0x01, 0xA5), DevInput.bytes("01 a5"))
        assertEquals(listOf(0x01, 0xA5), DevInput.bytes("01:A5"))
        assertEquals(emptyList<Int>(), DevInput.bytes(""))
        assertNull(DevInput.bytes("1"))
        assertNull(DevInput.bytes("zz"))
        assertNull(DevInput.bytes("00".repeat(KumiLimits.BYTE_BLOCK + 1)))
        assertEquals(KumiLimits.BYTE_BLOCK, DevInput.readCount("64"))
        assertNull(DevInput.readCount("65"))
        assertEquals("03 41", DevInput.hex(listOf(0x03, 0x41)))
    }
}
