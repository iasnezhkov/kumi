// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.protocol

data class WriteOutcome(val acknowledged: Boolean, val readBackMatches: Boolean?)

class SnapCase(private val transport: Transport, private val writesAllowed: () -> Boolean = { false }) {

    fun exchange(request: ByteArray): Reply {
        val outgoing = SnapProtocol.decodeReply(request)
        if (outgoing.group == SnapProtocol.GROUP_SET) {
            if (outgoing.command in SnapProtocol.FORBIDDEN_SET_COMMANDS) {
                throw ProtocolException("Command %02x is never sent".format(outgoing.command))
            }
            if (!writesAllowed()) throw WritesDisabledException()
        }
        val raw = transport.exchange(request)
        val reply = SnapProtocol.decodeReply(raw)
        if (!SnapProtocol.matches(request, raw)) throw ProtocolException("Reply group or command does not match request")
        return reply
    }

    fun get(command: Int, data: ByteArray = ByteArray(0)): Reply = exchange(SnapProtocol.encodeGet(command, data))

    fun version(): String = SnapProtocol.decodeVersion(get(SnapProtocol.CMD_VERSION))

    fun recordGainRaw(): Int = SnapProtocol.decodeInt16Getter(get(SnapProtocol.CMD_RECORD_GAIN), SnapProtocol.CMD_RECORD_GAIN)

    fun globalGainRaw(): Int = SnapProtocol.decodeInt16Getter(get(SnapProtocol.CMD_GLOBAL_GAIN), SnapProtocol.CMD_GLOBAL_GAIN)

    fun readEqBand(bank: Int, band: Int): EqBand {
        val result = EqCodec.decodeReply(exchange(EqCodec.encodeRead(bank, band)).frame)
        if (result.bank != bank || result.band != band) throw ProtocolException("EQ reply is for another slot")
        return result
    }

    fun readEqBank(bank: Int): List<EqBand> = (0 until EqCodec.BANDS).map { readEqBand(bank, it) }

    fun readMemory(address: Long, words: Int = 1): LongArray {
        if (words !in 1..SnapProtocol.MAX_MEMORY_WORDS) throw ProtocolException("Word count must be 1..${SnapProtocol.MAX_MEMORY_WORDS}")
        val batch = maxOf(words, MIN_READ_WORDS)
        val request = SnapProtocol.encodeMemoryRead(address, batch)
        val raw = transport.exchange(request)
        if (!SnapProtocol.matches(request, raw)) throw ProtocolException("Reply group or command does not match request")
        return SnapProtocol.decodeMemoryRead(raw, batch).copyOf(words)
    }

    fun readWords(wordAddresses: Collection<Long>): WordImage {
        val words = HashMap<Long, Long>()
        for (run in MemoryPlanner.plan(wordAddresses)) {
            readMemory(run.address, run.words).forEachIndexed { index, value -> words[run.address + 4L * index] = value }
        }
        return WordImage(words)
    }

    fun readLiveState(bank: Int): LiveState = LiveStateMap.decode(bank, readWords(LiveStateMap.wordAddresses(bank)))

    fun readAudioState(): CaseAudioState = StockState.decode(readWords(StockState.wordAddresses()))

    fun writeDacFilter(filter: DacFilter): WriteOutcome = writeRamByte(StockState.DAC_FILTER, filter.wire)

    fun writeRamByte(address: Long, value: Int): WriteOutcome {
        if (address !in StockState.TWEAK_ALLOWLIST) throw ProtocolException("Address %08x is not an allowed tweak".format(address))
        if (value !in 0..0xFF) throw ProtocolException("Byte value must be 0..255")
        if (!writesAllowed()) throw WritesDisabledException()
        try {
            version()
        } catch (error: Exception) {
            throw CaseAsleepException(error)
        }
        val wordAddress = address and 3L.inv()
        val shift = 8 * (address and 3L).toInt()
        val before = readMemory(wordAddress)[0]
        val merged = (before and (0xFFL shl shift).inv()) or (value.toLong() shl shift)
        transport.sendOnly(SnapProtocol.encodeMemoryWrite(wordAddress, merged))
        settle()
        val after = readMemory(wordAddress)[0]
        val readBack = ((after shr shift) and 0xFFL).toInt()
        return WriteOutcome(true, readBack == value)
    }

    fun flashMagicMatches(): Boolean =
        readMemory(SnapProtocol.FLASH_MAGIC_ADDRESS)[0] == SnapProtocol.FLASH_MAGIC_VALUE

    fun writeEqBand(band: EqBand): WriteOutcome {
        val acknowledged = guardedSet(EqCodec.encodeWrite(band))
        val readBack = runCatching { readEqBand(band.bank, band.band) }.getOrNull()
        return WriteOutcome(acknowledged, readBack?.sameSettingsAs(band))
    }

    fun writeRecordGain(db: Double): WriteOutcome {
        val raw = SnapProtocol.recordGainRaw(db)
        val acknowledged = guardedSet(SnapProtocol.encodeRecordGain(raw))
        val readBack = runCatching { recordGainRaw() }.getOrNull()
        return WriteOutcome(acknowledged, readBack?.let { it == SnapProtocol.quantizeRecordGain(raw) })
    }

    fun enterServiceMode() {
        try {
            version()
        } catch (error: Exception) {
            throw CaseAsleepException(error)
        }
        transport.sendOnly(SnapProtocol.encodeServiceEntry())
    }

    fun applyEq(): WriteOutcome {
        val sent = guardedSet(EqCodec.encodeApply())
        val word = runCatching { readMemory(EQ_ENABLE_MASK)[0] }.getOrNull()
        return WriteOutcome(sent, word?.let { it == APPLIED_MASK })
    }

    private fun guardedSet(request: ByteArray): Boolean {
        if (!writesAllowed()) throw WritesDisabledException()
        val command = request[2].toInt() and 0xFF
        if (command in SnapProtocol.FORBIDDEN_SET_COMMANDS) throw ProtocolException("Command %02x is never sent".format(command))
        try {
            version()
        } catch (error: Exception) {
            throw CaseAsleepException(error)
        }
        transport.sendOnly(request)
        settle()
        return true
    }

    private fun settle() {
        Thread.sleep(SETTLE_MS)
    }

    companion object {
        const val MIN_READ_WORDS = 2
        const val SETTLE_MS = 12L
        const val EQ_ENABLE_MASK = 0x00113A54L
        const val APPLIED_MASK = 0x0000FFFFL
    }
}
