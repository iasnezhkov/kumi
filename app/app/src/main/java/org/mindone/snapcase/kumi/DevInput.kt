// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

object DevInput {
    fun word(text: String): Long? {
        val digits = text.trim().removePrefix("0x").removePrefix("0X")
        if (digits.isEmpty() || digits.length > 8) return null
        return digits.toLongOrNull(16)
    }

    fun address(text: String): Long? = word(text)?.takeIf { it % 4 == 0L }

    fun i2cAddress(text: String): Int? = word(text)?.takeIf { it <= KumiLimits.I2C_MAX_ADDRESS }?.toInt()

    fun bytes(text: String): List<Int>? {
        val digits = text.filterNot { it.isWhitespace() || it == ':' || it == ',' }.removePrefix("0x")
        if (digits.length % 2 != 0 || digits.length / 2 > KumiLimits.BYTE_BLOCK) return null
        return digits.chunked(2).map { it.toIntOrNull(16) ?: return null }
    }

    fun readCount(text: String): Int? = text.trim().toIntOrNull()?.takeIf { it in 0..KumiLimits.BYTE_BLOCK }

    fun hex(bytes: List<Int>): String = bytes.joinToString(" ") { "%02x".format(it) }

    fun wordText(value: Long): String = "%08x".format(value)
}
