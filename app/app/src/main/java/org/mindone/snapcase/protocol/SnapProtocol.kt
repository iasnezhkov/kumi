// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.protocol

class ProtocolException(message: String) : IllegalArgumentException(message)

class Reply(val group: Int, val command: Int, val frame: ByteArray) {
    val payload: ByteArray get() = frame.copyOfRange(3, frame.size)

    fun u8(index: Int): Int = frame.u8(index)

    fun s16(index: Int): Int = frame.s16le(index)
}

object SnapProtocol {
    const val REPORT_SIZE = 64
    const val MAGIC = 0x4B
    const val GROUP_SET = 0x01
    const val GROUP_GET = 0x80
    const val CMD_MEMORY = 0x00
    const val CMD_SAVE = 0x01
    const val CMD_SAVE_ALT = 0x04
    const val CMD_RECORD_GAIN = 0x02
    const val CMD_GLOBAL_GAIN = 0x03
    const val CMD_MODE = 0x05
    const val CMD_EQ = 0x09
    const val CMD_EQ_APPLY = 0x0A
    const val CMD_VERSION = 0x0C
    const val CMD_IDENTITY = 0x4C
    const val CMD_RESET = 0xFF
    const val MAX_DATA = REPORT_SIZE - 4
    const val MAX_MEMORY_WORDS = 14
    const val FLASH_MAGIC_ADDRESS = 0x48000004L
    const val FLASH_MAGIC_VALUE = 0x40565053L
    const val RECORD_GAIN_MIN_DB = -24.0
    const val RECORD_GAIN_MAX_DB = 12.0
    const val RECORD_GAIN_STEP_RAW = 384

    val FORBIDDEN_SET_COMMANDS = setOf(CMD_RESET, CMD_MEMORY, CMD_SAVE, CMD_SAVE_ALT, CMD_MODE)

    fun encodeCommand(group: Int, command: Int, data: ByteArray = ByteArray(0)): ByteArray {
        requireUint(group, 8, "group")
        requireUint(command, 8, "command")
        if (group != GROUP_SET && group != GROUP_GET) throw ProtocolException("Unsupported normal-mode group")
        if (data.size > MAX_DATA) throw ProtocolException("Data exceeds $MAX_DATA bytes")
        val frame = ByteArray(REPORT_SIZE)
        frame[0] = MAGIC.toByte()
        frame[1] = group.toByte()
        frame[2] = command.toByte()
        frame[3] = data.size.toByte()
        data.copyInto(frame, 4)
        return frame
    }

    fun decodeReply(frame: ByteArray): Reply {
        if (frame.size !in 4..REPORT_SIZE || frame.u8(0) != MAGIC) {
            throw ProtocolException("Invalid normal-mode reply header or size")
        }
        val group = frame.u8(1)
        if (group != GROUP_SET && group != GROUP_GET) throw ProtocolException("Unexpected reply group")
        return Reply(group, frame.u8(2), frame.copyOf())
    }

    fun matches(request: ByteArray, reply: ByteArray): Boolean =
        reply.size >= 3 && request.size >= 3 &&
            reply[0] == request[0] && reply[1] == request[1] && reply[2] == request[2]

    fun encodeGet(command: Int, data: ByteArray = ByteArray(0)) = encodeCommand(GROUP_GET, command, data)

    fun encodeSet(command: Int, data: ByteArray = ByteArray(0)) = encodeCommand(GROUP_SET, command, data)

    fun decodeVersion(reply: Reply): String {
        expect(reply, GROUP_GET, CMD_VERSION)
        val text = StringBuilder()
        for (index in 4 until reply.frame.size) {
            val value = reply.u8(index)
            if (value == 0) break
            if (value !in 0x20..0x7E) throw ProtocolException("Non-printable version byte")
            text.append(value.toChar())
        }
        if (text.isEmpty()) throw ProtocolException("Empty version reply")
        return text.toString()
    }

    fun decodeInt16Getter(reply: Reply, command: Int): Int {
        expect(reply, GROUP_GET, command)
        if (reply.frame.size < 6) throw ProtocolException("Incomplete int16 reply")
        return reply.s16(4)
    }

    fun encodeRecordGain(raw: Int): ByteArray {
        if (raw !in Short.MIN_VALUE..Short.MAX_VALUE) throw ProtocolException("Gain must fit int16")
        val data = ByteArray(2)
        data.putU16le(0, raw)
        return encodeSet(CMD_RECORD_GAIN, data)
    }

    fun recordGainRaw(db: Double): Int {
        if (db.isNaN() || db < RECORD_GAIN_MIN_DB || db > RECORD_GAIN_MAX_DB) {
            throw ProtocolException("Record gain must be $RECORD_GAIN_MIN_DB..$RECORD_GAIN_MAX_DB dB")
        }
        return Math.round(db * 256).toInt()
    }

    fun quantizeRecordGain(raw: Int): Int {
        val biased = if (raw > 0) raw + RECORD_GAIN_STEP_RAW / 2 else raw - RECORD_GAIN_STEP_RAW / 2
        return (biased.toShort().toInt() / RECORD_GAIN_STEP_RAW) * RECORD_GAIN_STEP_RAW
    }

    fun encodeServiceEntry(): ByteArray {
        val frame = ByteArray(REPORT_SIZE)
        frame[0] = MAGIC.toByte()
        frame[1] = GROUP_SET.toByte()
        frame[2] = CMD_RESET.toByte()
        return frame
    }

    fun encodeIdentity(): ByteArray = encodeCommand(GROUP_SET, CMD_IDENTITY, ByteArray(8))

    fun decodeIdentity(frame: ByteArray): Long {
        val reply = decodeReply(frame)
        expect(reply, GROUP_GET, CMD_IDENTITY)
        if (frame.size < 12 || frame.u8(3) != 8) throw ProtocolException("Incomplete identity reply")
        return frame.u32le(8)
    }

    fun encodeMemoryRead(address: Long, words: Int = 1): ByteArray {
        requireAddress(address)
        if (words !in 1..MAX_MEMORY_WORDS) throw ProtocolException("Word count must be 1..$MAX_MEMORY_WORDS")
        val frame = ByteArray(REPORT_SIZE)
        frame[0] = MAGIC.toByte()
        frame[1] = GROUP_GET.toByte()
        frame[2] = CMD_MEMORY.toByte()
        frame[3] = (4 + 4 * words).toByte()
        frame.putU32le(4, address)
        return frame
    }

    fun decodeMemoryRead(frame: ByteArray, words: Int = 1): LongArray {
        if (words !in 1..MAX_MEMORY_WORDS) throw ProtocolException("Word count must be 1..$MAX_MEMORY_WORDS")
        val reply = decodeReply(frame)
        expect(reply, GROUP_GET, CMD_MEMORY)
        if (frame.size < 8 + 4 * words) throw ProtocolException("Incomplete raw memory read reply")
        return LongArray(words) { frame.u32le(8 + 4 * it) }
    }

    fun encodeMemoryWrite(address: Long, value: Long): ByteArray {
        requireAddress(address)
        requireAddress(value)
        val data = ByteArray(8)
        data.putU32le(0, address)
        data.putU32le(4, value)
        return encodeCommand(GROUP_SET, CMD_MEMORY, data)
    }

    fun expect(reply: Reply, group: Int, command: Int) {
        if (reply.group != group || reply.command != command) {
            throw ProtocolException(
                "Unexpected reply %02x/%02x, expected %02x/%02x".format(reply.group, reply.command, group, command),
            )
        }
    }

    private fun requireUint(value: Int, bits: Int, name: String) {
        if (value < 0 || value >= (1 shl bits)) throw ProtocolException("Expected uint$bits for $name")
    }

    private fun requireAddress(value: Long) {
        if (value < 0 || value > 0xFFFFFFFFL) throw ProtocolException("Expected uint32")
    }
}

fun ByteArray.u8(index: Int): Int = this[index].toInt() and 0xFF

fun ByteArray.u16le(index: Int): Int = u8(index) or (u8(index + 1) shl 8)

fun ByteArray.s16le(index: Int): Int = u16le(index).toShort().toInt()

fun ByteArray.s32le(index: Int): Int =
    u8(index) or (u8(index + 1) shl 8) or (u8(index + 2) shl 16) or (u8(index + 3) shl 24)

fun ByteArray.u32le(index: Int): Long = s32le(index).toLong() and 0xFFFFFFFFL

fun ByteArray.putU16le(index: Int, value: Int) {
    this[index] = value.toByte()
    this[index + 1] = (value shr 8).toByte()
}

fun ByteArray.putU32le(index: Int, value: Long) {
    for (shift in 0..3) this[index + shift] = (value shr (8 * shift)).toByte()
}

fun ByteArray.toHex(limit: Int = size): String =
    take(limit).joinToString("") { "%02x".format(it.toInt() and 0xFF) }

fun hexToBytes(text: String): ByteArray {
    val clean = text.filterNot { it.isWhitespace() }
    if (clean.length % 2 != 0) throw ProtocolException("Odd hex length")
    return ByteArray(clean.length / 2) { clean.substring(2 * it, 2 * it + 2).toInt(16).toByte() }
}
