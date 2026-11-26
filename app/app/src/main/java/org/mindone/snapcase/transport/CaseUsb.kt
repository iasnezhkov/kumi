// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.transport

enum class CaseKind { NONE, STOCK, SERVICE, KUMI }

data class UsbId(val vendorId: Int, val productId: Int) {
    fun matches(vendorId: Int, productId: Int): Boolean = this.vendorId == vendorId && this.productId == productId

    override fun toString(): String = "%04x:%04x".format(vendorId, productId)
}

object CaseUsb {
    val STOCK = UsbId(0x0661, 0x0881)
    val SERVICE = UsbId(0x0666, 0x0888)
    val KUMI = UsbId(0x1209, 0x0001)

    private val table = linkedMapOf(CaseKind.KUMI to KUMI, CaseKind.SERVICE to SERVICE, CaseKind.STOCK to STOCK)

    fun id(kind: CaseKind): UsbId? = table[kind]

    fun kindOf(vendorId: Int, productId: Int): CaseKind =
        table.entries.firstOrNull { it.value.matches(vendorId, productId) }?.key ?: CaseKind.NONE

    fun isKeyboard(vendorId: Int, productId: Int): Boolean = kindOf(vendorId, productId).let { it == CaseKind.STOCK || it == CaseKind.KUMI }

    fun strongest(kinds: Collection<CaseKind>): CaseKind = table.keys.firstOrNull { it in kinds } ?: CaseKind.NONE
}
