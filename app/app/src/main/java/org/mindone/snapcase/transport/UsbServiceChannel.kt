// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.transport

import android.hardware.usb.UsbConstants
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbEndpoint
import android.hardware.usb.UsbInterface
import android.hardware.usb.UsbManager
import java.io.IOException
import org.mindone.snapcase.protocol.CaseException
import org.mindone.snapcase.protocol.SnapProtocol

class UsbServiceChannel private constructor(
    private var connection: UsbDeviceConnection?,
    private val usbInterface: UsbInterface,
    private val input: UsbEndpoint,
    private val output: UsbEndpoint,
) : CaseChannel {
    private var reply: ByteArray? = null

    @Synchronized
    override fun send(data: ByteArray): Boolean {
        reply = null
        val link = connection ?: return false
        if (data.size != PACKET_SIZE) return false
        if (link.bulkTransfer(output, data, data.size, TIMEOUT_MS) != data.size) return false
        val isErase = data.size >= 6 && data[4] == 0x20.toByte() && data[5] == 0x01.toByte()
        val readTimeout = if (isErase) ERASE_TIMEOUT_MS else TIMEOUT_MS
        val buffer = ByteArray(PACKET_SIZE)
        val read = link.bulkTransfer(input, buffer, buffer.size, readTimeout)
        if (read <= 0) return false
        reply = buffer.copyOf(read)
        return true
    }

    @Synchronized
    override fun receive(): ByteArray? = reply.also { reply = null }

    @Synchronized
    override fun close() {
        connection?.let {
            runCatching { it.releaseInterface(usbInterface) }
            runCatching { it.close() }
        }
        connection = null
    }

    companion object {
        const val PACKET_SIZE = 512
        private const val TIMEOUT_MS = 1500
        private const val ERASE_TIMEOUT_MS = 20_000

        fun open(manager: UsbManager, device: UsbDevice): UsbServiceChannel {
            if (!CaseUsb.SERVICE.matches(device.vendorId, device.productId)) throw IOException("Expected the service device ${CaseUsb.SERVICE}")
            val match = (0 until device.interfaceCount).map(device::getInterface).firstNotNullOfOrNull { candidate ->
                if (candidate.id != 0 || candidate.interfaceClass != UsbConstants.USB_CLASS_HID) return@firstNotNullOfOrNull null
                val endpoints = (0 until candidate.endpointCount).map(candidate::getEndpoint)
                val input = endpoints.firstOrNull { it.address == 0x81 && it.type == UsbConstants.USB_ENDPOINT_XFER_INT }
                val output = endpoints.firstOrNull { it.address == 0x02 && it.type == UsbConstants.USB_ENDPOINT_XFER_INT }
                if (input != null && output != null) Triple(candidate, input, output) else null
            } ?: throw IOException("Expected HID interface 0 with interrupt endpoints 81/02")
            val connection = manager.openDevice(device) ?: throw IOException("Cannot open the service device")
            if (!connection.claimInterface(match.first, true)) {
                connection.close()
                throw IOException("Cannot claim the service interface")
            }
            return UsbServiceChannel(connection, match.first, match.second, match.third)
        }
    }
}

object UsbHidEntry {
    fun sendServiceEntry(manager: UsbManager, device: UsbDevice) {
        if (!CaseUsb.STOCK.matches(device.vendorId, device.productId)) throw CaseException("Expected the stock case ${CaseUsb.STOCK}")
        val match = (0 until device.interfaceCount).map(device::getInterface).firstNotNullOfOrNull { candidate ->
            if (candidate.interfaceClass != UsbConstants.USB_CLASS_HID) return@firstNotNullOfOrNull null
            (0 until candidate.endpointCount).map(candidate::getEndpoint)
                .firstOrNull { it.direction == UsbConstants.USB_DIR_OUT && it.type == UsbConstants.USB_ENDPOINT_XFER_INT }
                ?.let { candidate to it }
        } ?: throw CaseException("No HID OUT endpoint on the case")
        val connection = manager.openDevice(device) ?: throw CaseException("Cannot open USB device")
        try {
            if (!connection.claimInterface(match.first, true)) throw CaseException("Cannot claim the HID interface")
            val frame = SnapProtocol.encodeServiceEntry()
            val written = connection.bulkTransfer(match.second, frame, frame.size, 1000)
            if (written != frame.size) throw CaseException("USB OUT failed: $written")
        } finally {
            runCatching { connection.releaseInterface(match.first) }
            connection.close()
        }
    }
}
