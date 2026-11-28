// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import android.hardware.usb.UsbConstants
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbEndpoint
import android.hardware.usb.UsbInterface
import android.hardware.usb.UsbManager
import org.mindone.snapcase.transport.CaseChannel
import org.mindone.snapcase.transport.CaseKind
import org.mindone.snapcase.transport.CaseUsb

object KumiUsb {
    const val INTERFACE_CLASS = 0xFF
    const val INTERFACE_SUBCLASS = 0x4B
    const val INTERFACE_PROTOCOL = 0x01

    fun isKumi(device: UsbDevice): Boolean =
        CaseUsb.kindOf(device.vendorId, device.productId) == CaseKind.KUMI && controlInterface(device) != null

    fun controlInterface(device: UsbDevice): UsbInterface? = (0 until device.interfaceCount).map(device::getInterface).firstOrNull {
        it.interfaceClass == INTERFACE_CLASS && it.interfaceSubclass == INTERFACE_SUBCLASS && it.interfaceProtocol == INTERFACE_PROTOCOL
    }
}

class UsbKumiLink private constructor(
    private var connection: UsbDeviceConnection?,
    private val usbInterface: UsbInterface,
    private val input: UsbEndpoint,
    private val output: UsbEndpoint,
) : CaseChannel {

    private val readBuffer = ByteArray(FRAME_LIMIT)
    private val emptyPacket = ByteArray(0)

    @Synchronized
    override fun send(data: ByteArray): Boolean {
        val link = connection ?: return false
        if (link.bulkTransfer(output, data, data.size, SEND_TIMEOUT_MS) != data.size) return false
        val maxPacket = output.maxPacketSize
        if (data.isNotEmpty() && maxPacket > 0 && data.size % maxPacket == 0) {
            if (link.bulkTransfer(output, emptyPacket, 0, SEND_TIMEOUT_MS) != 0) return false
        }
        return true
    }

    @Synchronized
    override fun receive(): ByteArray? = readWithTimeout(RECEIVE_TIMEOUT_MS)

    @Synchronized
    override fun tryReceive(): ByteArray? = readWithTimeout(TRY_TIMEOUT_MS)

    private fun readWithTimeout(timeoutMs: Int): ByteArray? {
        val link = connection ?: return null
        val read = link.bulkTransfer(input, readBuffer, readBuffer.size, timeoutMs)
        return if (read > 0) readBuffer.copyOf(read) else null
    }

    @Synchronized
    override fun close() {
        connection?.let {
            runCatching { it.releaseInterface(usbInterface) }
            runCatching { it.close() }
        }
        connection = null
    }

    companion object {
        private const val SEND_TIMEOUT_MS = 3000
        private const val RECEIVE_TIMEOUT_MS = 5000
        private const val TRY_TIMEOUT_MS = 10
        private const val FRAME_LIMIT = 4096

        fun open(manager: UsbManager, device: UsbDevice): UsbKumiLink {
            val usbInterface = KumiUsb.controlInterface(device) ?: throw IllegalStateException("No Kumi control interface")
            val endpoints = (0 until usbInterface.endpointCount).map(usbInterface::getEndpoint).filter { it.type == UsbConstants.USB_ENDPOINT_XFER_BULK }
            val input = endpoints.firstOrNull { it.direction == UsbConstants.USB_DIR_IN } ?: throw IllegalStateException("No bulk IN endpoint")
            val output = endpoints.firstOrNull { it.direction == UsbConstants.USB_DIR_OUT } ?: throw IllegalStateException("No bulk OUT endpoint")
            val connection = manager.openDevice(device) ?: throw IllegalStateException("Cannot open the Kumi case")
            if (!connection.claimInterface(usbInterface, false)) {
                connection.close()
                throw IllegalStateException("Cannot claim the Kumi control interface")
            }
            return UsbKumiLink(connection, usbInterface, input, output)
        }
    }
}

class AssetPackages(private val context: android.content.Context) : BundledPackages {
    override fun names(): List<String> = context.assets.list(FirmwareCatalog.ASSET_DIR)?.toList() ?: emptyList()
    override fun open(path: String): java.io.InputStream = context.assets.open(path)
}
