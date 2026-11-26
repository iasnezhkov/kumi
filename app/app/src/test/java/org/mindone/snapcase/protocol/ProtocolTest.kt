// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.protocol

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class ProtocolTest {

    private fun hex(text: String, size: Int = 64) = hexToBytes(text).copyOf(size)

    @Test
    fun commandVectorsAndPayloadPreservation() {
        for (group in listOf(SnapProtocol.GROUP_SET, SnapProtocol.GROUP_GET)) {
            for (data in listOf(ByteArray(0), byteArrayOf(0, 0xFF.toByte()), ByteArray(60) { it.toByte() })) {
                val frame = SnapProtocol.encodeCommand(group, 0x31, data)
                assertEquals(64, frame.size)
                assertArrayEquals(byteArrayOf(0x4B, group.toByte(), 0x31, data.size.toByte()), frame.copyOfRange(0, 4))
                assertArrayEquals(data, frame.copyOfRange(4, 4 + data.size))
                assertArrayEquals(ByteArray(60 - data.size), frame.copyOfRange(4 + data.size, 64))
                val reply = SnapProtocol.decodeReply(frame)
                assertEquals(group, reply.group)
                assertEquals(0x31, reply.command)
                assertArrayEquals(frame.copyOfRange(3, 64), reply.payload)
            }
        }
    }

    @Test
    fun replyLayoutIsNotInferredFromByteThree() {
        val raw = hexToBytes("4b800c") + "V0.1".toByteArray() + byteArrayOf(0)
        assertArrayEquals("V0.1".toByteArray() + byteArrayOf(0), SnapProtocol.decodeReply(raw).payload)
        val prefixed = hexToBytes("4b800c04") + "V0.1".toByteArray()
        assertArrayEquals(byteArrayOf(4) + "V0.1".toByteArray(), SnapProtocol.decodeReply(prefixed).payload)
    }

    @Test
    fun versionFromDeviceCaptureAndSnapkitVector() {
        val device = hex("4b800c0056302e31")
        assertEquals("V0.1", SnapProtocol.decodeVersion(SnapProtocol.decodeReply(device)))
        val snapkit = hex("4b800c04") .also { "V0.1".toByteArray().copyInto(it, 4) }
        assertEquals("V0.1", SnapProtocol.decodeVersion(SnapProtocol.decodeReply(snapkit)))
        assertThrows(ProtocolException::class.java) { SnapProtocol.decodeVersion(SnapProtocol.decodeReply(hex("4b800c00"))) }
        assertThrows(ProtocolException::class.java) { SnapProtocol.decodeVersion(SnapProtocol.decodeReply(hex("4b800200560a"))) }
    }

    @Test
    fun invalidRequests() {
        val cases = listOf(
            Triple(0, 2, ByteArray(0)),
            Triple(-1, 2, ByteArray(0)),
            Triple(0x80, 256, ByteArray(0)),
            Triple(0x80, -1, ByteArray(0)),
            Triple(0x80, 2, ByteArray(61)),
            Triple(0x02, 2, ByteArray(0)),
        )
        for ((group, command, data) in cases) {
            assertThrows(ProtocolException::class.java) { SnapProtocol.encodeCommand(group, command, data) }
        }
    }

    @Test
    fun invalidReplies() {
        val frames = listOf(
            ByteArray(0),
            hexToBytes("4b8002"),
            hexToBytes("00800200"),
            hexToBytes("4b020200"),
            hexToBytes("4b8002") + ByteArray(62),
        )
        for (frame in frames) {
            assertThrows(ProtocolException::class.java) { SnapProtocol.decodeReply(frame) }
        }
    }

    @Test
    fun identityVectors() {
        assertArrayEquals(hex("4b014c08"), SnapProtocol.encodeIdentity())
        val frame = hexToBytes("4b804c080000000080440149")
        assertEquals(0x49014480L, SnapProtocol.decodeIdentity(frame))
        for (bad in listOf(frame.copyOf(11), hexToBytes("4b804c07") + frame.copyOfRange(4, 12), hexToBytes("4b014c08") + frame.copyOfRange(4, 12))) {
            assertThrows(ProtocolException::class.java) { SnapProtocol.decodeIdentity(bad) }
        }
    }

    @Test
    fun rawMemoryVectors() {
        assertArrayEquals(hex("4b80000834120048"), SnapProtocol.encodeMemoryRead(0x48001234L))
        assertArrayEquals(hex("4b01000834120048efbeadde"), SnapProtocol.encodeMemoryWrite(0x48001234L, 0xDEADBEEFL))
        val frame = hexToBytes("4b80000834120048efbeadde")
        assertEquals(0xDEADBEEFL, SnapProtocol.decodeMemoryRead(frame)[0])
        for (bad in listOf(frame.copyOf(11), hexToBytes("4b800108") + frame.copyOfRange(4, 12), hexToBytes("4b010008") + frame.copyOfRange(4, 12))) {
            assertThrows(ProtocolException::class.java) { SnapProtocol.decodeMemoryRead(bad) }
        }
    }

    @Test
    fun memoryReadOfSeveralWords() {
        val request = SnapProtocol.encodeMemoryRead(0x00100000L, 14)
        assertEquals(60, request[3].toInt())
        assertThrows(ProtocolException::class.java) { SnapProtocol.encodeMemoryRead(0, 15) }
        assertThrows(ProtocolException::class.java) { SnapProtocol.encodeMemoryRead(0, 0) }
        val reply = request.copyOf()
        for (index in 0 until 14) reply.putU32le(8 + 4 * index, 0x01020300L + index)
        val words = SnapProtocol.decodeMemoryRead(reply, 14)
        assertEquals(14, words.size)
        assertEquals(0x0102030DL, words[13])
        assertThrows(ProtocolException::class.java) { SnapProtocol.decodeMemoryRead(reply.copyOf(60), 14) }
    }

    @Test
    fun memoryReadDeviceCapture() {
        assertArrayEquals(hex("4b80000804000048"), SnapProtocol.encodeMemoryRead(SnapProtocol.FLASH_MAGIC_ADDRESS))
        val reply = hex("4b80000804000048535056400000")
        assertEquals(SnapProtocol.FLASH_MAGIC_VALUE, SnapProtocol.decodeMemoryRead(reply)[0])
    }

    @Test
    fun uint32Boundaries() {
        for (value in listOf(0L, 0xFFFFFFFFL)) {
            val read = SnapProtocol.encodeMemoryRead(value)
            assertEquals(value, read.u32le(4))
            val write = SnapProtocol.encodeMemoryWrite(value, value)
            assertEquals(value, write.u32le(4))
            assertEquals(value, write.u32le(8))
        }
        for (value in listOf(-1L, 0x100000000L)) {
            assertThrows(ProtocolException::class.java) { SnapProtocol.encodeMemoryRead(value) }
            assertThrows(ProtocolException::class.java) { SnapProtocol.encodeMemoryWrite(0, value) }
            assertThrows(ProtocolException::class.java) { SnapProtocol.encodeMemoryWrite(value, 0) }
        }
    }

    @Test
    fun int16Getters() {
        val record = SnapProtocol.decodeReply(hex("4b8002020000"))
        assertEquals(0, SnapProtocol.decodeInt16Getter(record, SnapProtocol.CMD_RECORD_GAIN))
        val negative = SnapProtocol.decodeReply(hex("4b80030200ff"))
        assertEquals(-256, SnapProtocol.decodeInt16Getter(negative, SnapProtocol.CMD_GLOBAL_GAIN))
        assertThrows(ProtocolException::class.java) {
            SnapProtocol.decodeInt16Getter(SnapProtocol.decodeReply(hexToBytes("4b800302")), SnapProtocol.CMD_GLOBAL_GAIN)
        }
        assertThrows(ProtocolException::class.java) { SnapProtocol.decodeInt16Getter(record, SnapProtocol.CMD_GLOBAL_GAIN) }
    }

    @Test
    fun replyMatchingUsesFirstThreeBytes() {
        val request = SnapProtocol.encodeGet(SnapProtocol.CMD_VERSION)
        assertTrue(SnapProtocol.matches(request, hex("4b800c00")))
        assertTrue(!SnapProtocol.matches(request, hex("4b800200")))
        assertTrue(!SnapProtocol.matches(request, hexToBytes("0400002c0000000000")))
    }
}
