// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.protocol

class DemoTransport : Transport {
    override val description = "Demo (scripted replies from measured captures)"
    override val isDemo = true

    @Volatile
    var asleep = false

    @Volatile
    var serviceRequested = false

    private var recordGain = 0

    val requests = mutableListOf<ByteArray>()
    private val bands = HashMap<Pair<Int, Int>, ByteArray>()
    private val written = HashMap<Long, Long>()

    init {
        MEASURED_BANK0.forEachIndexed { band, hex -> bands[0 to band] = pad(hexToBytes(hex)) }
        for (bank in 1 until EqCodec.BANKS) {
            for (band in 0 until EqCodec.BANDS) {
                val frame = bands.getValue(0 to band).copyOf()
                frame[4] = bank.toByte()
                frame.putU16le(32, DEFAULT_GAINS[bank][band])
                frame[35] = DEFAULT_PREAMPS[bank].toByte()
                bands[bank to band] = frame
            }
        }
    }

    @Synchronized
    override fun exchange(request: ByteArray, timeoutMs: Long): ByteArray {
        if (request.size != SnapProtocol.REPORT_SIZE) throw ProtocolException("Expected a 64-byte request")
        requests += request.copyOf()
        if (asleep) throw CaseTimeoutException("Demo case is asleep")
        val group = request.u8(1)
        val command = request.u8(2)
        return when {
            group == SnapProtocol.GROUP_GET && command == SnapProtocol.CMD_VERSION ->
                pad(hexToBytes("4b800c00") + "V0.1".toByteArray())
            group == SnapProtocol.GROUP_GET && command == SnapProtocol.CMD_RECORD_GAIN ->
                pad(hexToBytes("4b800202") + byteArrayOf(recordGain.toByte(), (recordGain shr 8).toByte()))
            group == SnapProtocol.GROUP_SET && command in DEMO_SET_COMMANDS -> {
                applySet(request)
                throw CaseTimeoutException("The case does not answer SET %02x".format(command))
            }
            group == SnapProtocol.GROUP_GET && command == SnapProtocol.CMD_GLOBAL_GAIN -> pad(hexToBytes("4b8003020000"))
            group == SnapProtocol.GROUP_GET && command == SnapProtocol.CMD_EQ -> {
                val key = request.u8(4) to request.u16le(5)
                bands[key]?.copyOf() ?: throw CaseTimeoutException("Demo has no such EQ slot")
            }
            group == SnapProtocol.GROUP_GET && command == SnapProtocol.CMD_MEMORY -> {
                val words = ((request.u8(3) - 4) / 4).coerceIn(1, SnapProtocol.MAX_MEMORY_WORDS)
                val address = request.u32le(4)
                val reply = request.copyOf()
                for (index in 0 until words) {
                    reply.putU32le(8 + 4 * index, memoryWord(address + 4L * index))
                }
                reply
            }
            else -> throw CaseTimeoutException("No PRIMARY dispatcher branch for %02x/%02x".format(group, command))
        }
    }

    @Synchronized
    override fun sendOnly(request: ByteArray) {
        if (request.size != SnapProtocol.REPORT_SIZE) throw ProtocolException("Expected a 64-byte request")
        requests += request.copyOf()
        if (asleep) throw CaseTimeoutException("Demo case is asleep")
        if (request.u8(1) != SnapProtocol.GROUP_SET) return
        if (request.u8(2) == SnapProtocol.CMD_RESET) serviceRequested = true else applySet(request)
    }

    private fun applySet(request: ByteArray) {
        when (request.u8(2)) {
            SnapProtocol.CMD_EQ -> {
                val key = request.u8(4) to request.u16le(5)
                val stored = request.copyOf()
                stored[1] = SnapProtocol.GROUP_GET.toByte()
                stored[3] = 4
                bands[key] = stored
            }
            SnapProtocol.CMD_MEMORY -> written[request.u32le(4)] = request.u32le(8)
            SnapProtocol.CMD_RECORD_GAIN -> recordGain = SnapProtocol.quantizeRecordGain(request.s16le(4))
            SnapProtocol.CMD_EQ_APPLY -> {
                val bank = request.u8(4)
                if (bank <= 8) written[SnapCase.EQ_ENABLE_MASK + 4L * bank] = request.u32le(4) shr 16
            }
        }
    }

    fun memoryWord(address: Long): Long {
        written[address]?.let { return it }
        MEMORY[address]?.let { return it }
        var word = 0L
        for (offset in 0..3) word = word or (memoryByte(address + offset).toLong() shl (8 * offset))
        return word
    }

    private fun memoryByte(address: Long): Int {
        tableByte(address, LiveStateMap.EQ_FREQUENCY) { _, band -> DEFAULT_FREQUENCIES[band] }?.let { return it }
        tableByte(address, LiveStateMap.EQ_Q) { _, _ -> DEFAULT_Q_RAW }?.let { return it }
        tableByte(address, LiveStateMap.EQ_GAIN) { bank, band -> DEFAULT_GAINS.getOrNull(bank)?.get(band) ?: 0 }?.let { return it }
        return MEMORY_BYTES[address] ?: 0
    }

    private fun tableByte(address: Long, base: Long, value: (Int, Int) -> Int): Int? {
        val offset = address - base
        if (offset < 0 || offset >= LiveStateMap.BANK_STRIDE * LiveStateMap.TABLE_BANKS) return null
        val slot = (offset / 2).toInt()
        val raw = value(slot / LiveStateMap.BANDS, slot % LiveStateMap.BANDS) and 0xFFFF
        return if (offset % 2 == 0L) raw and 0xFF else raw shr 8
    }

    private fun pad(bytes: ByteArray): ByteArray = bytes.copyOf(SnapProtocol.REPORT_SIZE)

    companion object {
        val MEASURED_BANK0 = listOf(
            "4b800904000000000000004098542c80a7bcd33f68abd37f59432cc02000b600000002fa00",
            "4b8009040001000000000040f6ac5880eb97a73f0a53a77f156858c04000b600000002fa00",
            "4b800904000200000000004047d7b280ef3f4e3fb9284d7f11c0b1c07d00b600000002fa00",
            "4b8009040003000000000040ece86581e06d9e3e14179a7e209261c1fa00b600000402fa00",
            "4b8009040004000000000040cfabcc82ae7f443d3154337d5280bbc2f401b600000402fa00",
            "4b800904000500000000004070499c85d1f0a63a90b6637a2f0f59c5e803b600000402fa00",
            "4b80090400060000000000400b06418bb4dec035f5f9be744c213fcad007b600000002fa00",
            "4b800904000700000000004098368e9641062a2d68c97169bff9d5d2a00fb600000202fa00",
        )

        val DEFAULT_GAINS = listOf(
            listOf(0, 0, 0, 1024, 1024, 1024, 0, 512),
            listOf(768, 256, 0, -384, -896, 896, 384, 0),
            listOf(384, 0, 512, 1024, -384, 384, 0, 0),
            listOf(384, 768, 1024, 256, -384, 384, 0, 0),
            listOf(205, 1613, 1229, 0, 435, 819, 1203, 1101),
            listOf(0, 1792, 1792, 1024, 0, 0, 0, 0),
            listOf(1229, 1075, 307, 717, -307, -307, -307, -256),
            listOf(0, 0, 0, 0, 0, 0, 0, 0),
            listOf(0, 0, 0, 0, 0, 0, 0, 0),
        )

        val DEFAULT_PREAMPS = listOf(-6, -3, -5, -5, -10, -11, -7, 0, 0)

        val DEFAULT_FREQUENCIES = listOf(32, 64, 125, 250, 500, 1000, 2000, 4000)
        const val DEFAULT_Q_RAW = 182

        val MEMORY_BYTES = mapOf(
            LiveStateMap.VOLUME to 60,
            LiveStateMap.VOLUME_FLAG to 1,
            0x00112894L to 2,
            LiveStateMap.RECORD_MARKER to 1,
            StockState.USB_CONFIG to 2,
            StockState.DAC_FILTER to 0,
            StockState.EQ_PROFILE to 0,
            StockState.SAMPLE_RATE to 0x80,
            StockState.SAMPLE_RATE + 1 to 0xBB,
            StockState.CAPTURE_TRIM to 0x80,
            StockState.CAPTURE_TRIM + 1 to 0x07,
        )

        val DEMO_SET_COMMANDS = setOf(SnapProtocol.CMD_EQ, SnapProtocol.CMD_MEMORY, SnapProtocol.CMD_RECORD_GAIN, SnapProtocol.CMD_EQ_APPLY)

        val MEMORY = mapOf(
            SnapCase.EQ_ENABLE_MASK to 0x0000FFFFL,
            0x48000000L to 0x0a40006fL,
            0x48000004L to 0x40565053L,
            0x48000020L to 0x00555043L,
        )
    }
}
