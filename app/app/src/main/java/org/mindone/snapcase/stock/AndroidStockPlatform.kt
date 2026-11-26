// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.stock

import android.content.Context
import org.mindone.snapcase.protocol.Transport
import org.mindone.snapcase.system.DeviceState
import org.mindone.snapcase.system.HidrawNode
import org.mindone.snapcase.system.Sysfs
import org.mindone.snapcase.system.TypecInfo
import org.mindone.snapcase.system.UsbDeviceInfo
import org.mindone.snapcase.system.UsbPresence
import org.mindone.snapcase.transport.CaseUsb
import org.mindone.snapcase.transport.HidrawTransport

class AndroidStockPlatform(private val context: Context, private val sysfs: Sysfs = Sysfs()) : StockPlatform {
    private val id = CaseUsb.STOCK

    override fun findNode(): HidrawNode? = sysfs.findHidraw(id.vendorId, id.productId)

    override fun usbInfo(): UsbDeviceInfo? = sysfs.findUsbDevice(id.vendorId, id.productId) ?: DeviceState.usbManagerInfo(context, id.vendorId, id.productId)

    override fun typec(): List<TypecInfo> = sysfs.typecPorts()

    override fun presence(): UsbPresence = DeviceState.usbPresence(context)

    override fun open(node: HidrawNode): Transport = HidrawTransport.open(node.devPath)
}
