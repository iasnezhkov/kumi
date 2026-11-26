// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.transport

import android.content.Context
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbManager
import org.mindone.snapcase.kumi.KumiUsb
import org.mindone.snapcase.kumi.UsbKumiLink

class UsbCasePort(private val context: Context, private val stockEntry: suspend () -> Boolean) : CasePort {
    private val usb: UsbManager? = context.getSystemService(UsbManager::class.java)

    override val simulated: Boolean = false

    override fun kind(): CaseKind = CaseUsb.strongest(usb?.deviceList?.values?.map(::kindOf) ?: emptyList())

    override fun deviceKey(): String? =
        usb?.deviceList?.values?.firstOrNull { kindOf(it) == CaseKind.KUMI }?.let { "${it.deviceName}:${it.deviceId}" }

    private fun kindOf(device: UsbDevice): CaseKind = when (val kind = CaseUsb.kindOf(device.vendorId, device.productId)) {
        CaseKind.KUMI -> if (KumiUsb.controlInterface(device) != null) kind else CaseKind.NONE
        else -> kind
    }

    private fun manager(): UsbManager = usb ?: throw IllegalStateException("USB host is not available")

    private fun find(kind: CaseKind): UsbDevice =
        usb?.deviceList?.values?.firstOrNull { kindOf(it) == kind } ?: throw IllegalStateException("No ${CaseUsb.id(kind)} device is attached")

    private suspend fun permit(device: UsbDevice) {
        if (!UsbPermission.request(context, manager(), device)) {
            throw PermissionDeniedException(CaseUsb.kindOf(device.vendorId, device.productId).let { CaseUsb.id(it) }?.toString() ?: device.deviceName)
        }
    }

    override suspend fun sendServiceEntry() {
        if (stockEntry()) return
        val device = find(CaseKind.STOCK)
        permit(device)
        UsbHidEntry.sendServiceEntry(manager(), device)
    }

    override suspend fun openService(): CaseChannel {
        val device = find(CaseKind.SERVICE)
        permit(device)
        return UsbServiceChannel.open(manager(), device)
    }

    override suspend fun openKumi(): CaseChannel {
        val device = find(CaseKind.KUMI)
        permit(device)
        return UsbKumiLink.open(manager(), device)
    }
}

object NoCasePort : CasePort {
    override val simulated: Boolean = true
    override fun kind(): CaseKind = CaseKind.NONE
    override suspend fun sendServiceEntry() = throw IllegalStateException("No case")
    override suspend fun openService(): CaseChannel = throw IllegalStateException("No case")
    override suspend fun openKumi(): CaseChannel = throw IllegalStateException("No case")
}
