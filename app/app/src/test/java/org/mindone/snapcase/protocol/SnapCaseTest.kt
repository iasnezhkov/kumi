// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.protocol

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class FakeTransport(private val responder: (ByteArray) -> ByteArray) : Transport {
    override val description = "fake"
    override val isDemo = true
    val requests = mutableListOf<ByteArray>()
    val sent = mutableListOf<ByteArray>()

    override fun exchange(request: ByteArray, timeoutMs: Long): ByteArray {
        requests += request.copyOf()
        return responder(request)
    }

    override fun sendOnly(request: ByteArray) {
        sent += request.copyOf()
    }
}

class SnapCaseTest {

    @Test
    fun readsThroughDemo() {
        val demo = DemoTransport()
        val case = SnapCase(demo)
        assertEquals("V0.1", case.version())
        assertEquals(0, case.recordGainRaw())
        assertEquals(0, case.globalGainRaw())
        val bank = case.readEqBank(0)
        assertEquals(8, bank.size)
        assertEquals(4000, bank[7].frequencyHz)
        assertArrayEquals(SnapProtocol.encodeGet(SnapProtocol.CMD_VERSION), demo.requests[0])
        assertTrue(case.flashMagicMatches())
    }

    @Test
    fun disabledWritesSendNothing() {
        val demo = DemoTransport()
        val case = SnapCase(demo) { false }
        val band = DemoTransport().let { SnapCase(it).readEqBand(0, 0) }
        assertThrows(WritesDisabledException::class.java) { case.writeEqBand(band) }
        assertThrows(WritesDisabledException::class.java) { case.applyEq() }
        assertTrue(demo.requests.isEmpty())
    }

    @Test
    fun sleepingCaseGetsOnlyTheVersionProbe() {
        val demo = DemoTransport()
        val band = SnapCase(demo).readEqBand(0, 1)
        demo.requests.clear()
        demo.asleep = true
        val case = SnapCase(demo) { true }
        assertThrows(CaseAsleepException::class.java) { case.writeEqBand(band.copy(gainRaw = 256)) }
        assertEquals(1, demo.requests.size)
        assertEquals(SnapProtocol.CMD_VERSION, demo.requests[0][2].toInt())
    }

    @Test
    fun eqWriteProbesWritesAndReadsBack() {
        val demo = DemoTransport()
        val case = SnapCase(demo) { true }
        val original = case.readEqBand(2, 5)
        demo.requests.clear()
        val changed = original.copy(gainRaw = -512, preamp = -4)
        val outcome = case.writeEqBand(changed)
        assertTrue(outcome.acknowledged)
        assertEquals(true, outcome.readBackMatches)
        assertEquals(listOf(0x80 to 0x0C, 0x01 to 0x09, 0x80 to 0x09), demo.requests.map { (it[1].toInt() and 0xFF) to it[2].toInt() })
        assertEquals(changed, case.readEqBand(2, 5))
    }

    @Test
    fun setIsSentWithoutWaitingAndCheckedByReadBack() {
        val transport = FakeTransport { request ->
            when (request[2].toInt() and 0xFF) {
                SnapProtocol.CMD_VERSION -> hexToBytes("4b800c0056302e31").copyOf(64)
                SnapProtocol.CMD_MEMORY -> request.copyOf().also { it.putU32le(8, 0x0000FFFFL) }
                else -> throw CaseTimeoutException("no reply")
            }
        }
        val outcome = SnapCase(transport) { true }.applyEq()
        assertTrue(outcome.acknowledged)
        assertEquals(true, outcome.readBackMatches)
        assertArrayEquals(hexToBytes("4b010a040000ffff").copyOf(64), transport.sent[0])
        assertEquals(listOf(0x0C, 0x00), transport.requests.map { it[2].toInt() and 0xFF })
        assertEquals(0x0C, transport.requests[1][3].toInt())
    }

    @Test
    fun flashWritingCommandsAreNeverSent() {
        val transport = FakeTransport { throw CaseTimeoutException("must not be sent") }
        val case = SnapCase(transport) { true }
        for (command in listOf(0x00, 0x01, 0x04, 0x05, 0xFF)) {
            assertThrows(ProtocolException::class.java) { case.exchange(SnapProtocol.encodeSet(command, byteArrayOf(2))) }
        }
        assertTrue(transport.requests.isEmpty())
    }

    @Test
    fun mismatchedReplyIsRejected() {
        for (reply in listOf(SnapProtocol.encodeGet(3), SnapProtocol.encodeSet(2))) {
            assertThrows(ProtocolException::class.java) { SnapCase(FakeTransport { reply }).get(2) }
        }
        val otherSlot = FakeTransport { hexToBytes(DemoTransport.MEASURED_BANK0[1]).copyOf(64) }
        assertThrows(ProtocolException::class.java) { SnapCase(otherSlot).readEqBand(0, 0) }
    }

    @Test
    fun demoHasNoBranchForUnimplementedCommands() {
        val case = SnapCase(DemoTransport())
        assertThrows(CaseTimeoutException::class.java) { case.get(0x22) }
        assertThrows(WritesDisabledException::class.java) { case.exchange(SnapProtocol.encodeIdentity()) }
    }

    @Test
    fun demoMemoryReadsSeveralWords() {
        val words = SnapCase(DemoTransport()).readMemory(0x48000000L, 3)
        assertEquals(listOf(0x0a40006fL, 0x40565053L, 0L), words.toList())
    }
}
