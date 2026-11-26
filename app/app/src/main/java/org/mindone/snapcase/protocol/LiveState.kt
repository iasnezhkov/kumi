// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.protocol

data class MemoryRun(val address: Long, val words: Int)

object MemoryPlanner {
    fun plan(wordAddresses: Collection<Long>, maxWords: Int = SnapProtocol.MAX_MEMORY_WORDS): List<MemoryRun> {
        if (maxWords !in 1..SnapProtocol.MAX_MEMORY_WORDS) throw ProtocolException("Bad batch size")
        val sorted = wordAddresses.toSortedSet()
        sorted.forEach { if (it % 4 != 0L || it !in 0..0xFFFFFFFCL) throw ProtocolException("Unaligned address") }
        val runs = mutableListOf<MemoryRun>()
        var start = -1L
        var count = 0
        for (address in sorted) {
            if (count > 0 && address == start + 4L * count && count < maxWords) {
                count++
            } else {
                if (count > 0) runs += MemoryRun(start, count)
                start = address
                count = 1
            }
        }
        if (count > 0) runs += MemoryRun(start, count)
        return runs
    }
}

class WordImage(private val words: Map<Long, Long>) {
    fun byteAt(address: Long): Int {
        val word = words[address and 3L.inv()] ?: throw ProtocolException("Word for %08x was not read".format(address))
        return ((word shr (8 * (address and 3L).toInt())) and 0xFF).toInt()
    }

    fun u8(address: Long): Int = byteAt(address)

    fun u16(address: Long): Int = byteAt(address) or (byteAt(address + 1) shl 8)

    fun s16(address: Long): Int = u16(address).toShort().toInt()

    fun u32(address: Long): Long {
        var value = 0L
        for (offset in 0..3) value = value or (byteAt(address + offset).toLong() shl (8 * offset))
        return value
    }
}

data class LiveState(
    val bank: Int,
    val volumeRaw: Int,
    val volumeFlag: Int,
    val getter02Shadow: Int,
    val getter03Shadow: Int,
    val recordMarker: Int,
    val frequencies: List<Int>,
    val qRaw: List<Int>,
    val gainRaw: List<Int>,
) {
    val looksLikeEcho: Boolean get() = frequencies.all { it == 0 } && qRaw.all { it == 0 }
}

object LiveStateMap {
    const val VOLUME = 0x00112893L
    const val VOLUME_FLAG = 0x0011288FL
    const val GETTER02_SHADOW = 0x00113A4CL
    const val GETTER03_SHADOW = 0x00113A4EL
    const val RECORD_MARKER = 0x00113A50L
    const val EQ_FREQUENCY = 0x001140BCL
    const val EQ_Q = 0x0011415CL
    const val EQ_GAIN = 0x001141FCL
    const val TABLE_BANKS = 10
    const val BANDS = 8
    const val BANK_STRIDE = 2L * BANDS

    fun tableAddress(base: Long, bank: Int, band: Int): Long {
        if (bank !in 0 until TABLE_BANKS || band !in 0 until BANDS) throw ProtocolException("Table slot out of range")
        return base + BANK_STRIDE * bank + 2L * band
    }

    fun byteAddresses(bank: Int): List<Long> {
        val tables = listOf(EQ_FREQUENCY, EQ_Q, EQ_GAIN).flatMap { base ->
            (0 until BANDS).flatMap { band -> tableAddress(base, bank, band).let { listOf(it, it + 1) } }
        }
        return listOf(VOLUME, VOLUME_FLAG, GETTER02_SHADOW, GETTER02_SHADOW + 1, GETTER03_SHADOW, GETTER03_SHADOW + 1,
            RECORD_MARKER, RECORD_MARKER + 1) + tables
    }

    fun wordAddresses(bank: Int): Set<Long> = byteAddresses(bank).map { it and 3L.inv() }.toSortedSet()

    fun decode(bank: Int, image: WordImage) = LiveState(
        bank = bank,
        volumeRaw = image.u8(VOLUME),
        volumeFlag = image.u8(VOLUME_FLAG),
        getter02Shadow = image.s16(GETTER02_SHADOW),
        getter03Shadow = image.s16(GETTER03_SHADOW),
        recordMarker = image.u16(RECORD_MARKER),
        frequencies = List(BANDS) { image.u16(tableAddress(EQ_FREQUENCY, bank, it)) },
        qRaw = List(BANDS) { image.u16(tableAddress(EQ_Q, bank, it)) },
        gainRaw = List(BANDS) { image.s16(tableAddress(EQ_GAIN, bank, it)) },
    )
}
