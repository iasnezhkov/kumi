// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.system

import java.io.File

data class HidrawNode(val name: String, val devPath: String, val hidName: String?, val hidPhys: String?)

data class UsbDeviceInfo(
    val sysfsName: String,
    val attributes: Map<String, String>,
) {
    val speedMbps: String? get() = attributes["speed"]
    val usbVersion: String? get() = attributes["version"]?.trim()
}

data class TypecInfo(
    val port: String,
    val powerRole: String?,
    val dataRole: String?,
    val powerOperationMode: String?,
    val partnerPresent: Boolean,
)

class Sysfs(private val root: File = File("/")) {

    fun findHidraw(vendorId: Int, productId: Int): HidrawNode? {
        val wanted = "HID_ID=0003:%08X:%08X".format(vendorId, productId)
        val base = File(root, "sys/class/hidraw")
        val nodes = base.listFiles()?.filter { it.name.startsWith("hidraw") }?.sortedBy { it.name } ?: return null
        for (node in nodes) {
            val uevent = parseUevent(readText(File(node, "device/uevent")) ?: continue)
            if (uevent["HID_ID"]?.let { "HID_ID=$it" }.equals(wanted, ignoreCase = true)) {
                return HidrawNode(node.name, "/dev/${node.name}", uevent["HID_NAME"], uevent["HID_PHYS"])
            }
        }
        return null
    }

    fun findUsbDevice(vendorId: Int, productId: Int): UsbDeviceInfo? {
        val base = File(root, "sys/bus/usb/devices")
        val vid = "%04x".format(vendorId)
        val pid = "%04x".format(productId)
        val dirs = base.listFiles()?.sortedBy { it.name } ?: return null
        for (dir in dirs) {
            if (readText(File(dir, "idVendor"))?.trim().equals(vid, true) &&
                readText(File(dir, "idProduct"))?.trim().equals(pid, true)
            ) {
                val attributes = USB_ATTRIBUTES.mapNotNull { name ->
                    readText(File(dir, name))?.trim()?.let { name to it }
                }.toMap()
                return UsbDeviceInfo(dir.name, attributes)
            }
        }
        return null
    }

    fun typecPorts(): List<TypecInfo> {
        val base = File(root, "sys/class/typec")
        val ports = base.listFiles()?.filter { PORT_NAME.matches(it.name) }?.sortedBy { it.name } ?: return emptyList()
        return ports.map { port ->
            TypecInfo(
                port = port.name,
                powerRole = readText(File(port, "power_role"))?.let(::selected),
                dataRole = readText(File(port, "data_role"))?.let(::selected),
                powerOperationMode = readText(File(port, "power_operation_mode"))?.trim(),
                partnerPresent = File(base, "${port.name}-partner").exists(),
            )
        }
    }

    private fun readText(file: File): String? = try {
        file.readText()
    } catch (ignored: Exception) {
        null
    }

    companion object {
        val USB_ATTRIBUTES = listOf(
            "manufacturer", "product", "serial", "speed", "version", "bcdDevice", "bMaxPower",
            "bmAttributes", "busnum", "devnum", "devpath", "removable", "power/runtime_status", "power/control",
        )
        private val PORT_NAME = Regex("port\\d+")

        fun parseUevent(text: String): Map<String, String> =
            text.lineSequence().mapNotNull { line ->
                val index = line.indexOf('=')
                if (index <= 0) null else line.substring(0, index).trim() to line.substring(index + 1).trim()
            }.toMap()

        fun selected(text: String): String {
            val match = Regex("\\[([^]]+)]").find(text)
            return match?.groupValues?.get(1) ?: text.trim()
        }

        fun selfPowered(bmAttributes: String?): Boolean? =
            bmAttributes?.trim()?.toIntOrNull(16)?.let { it and 0x40 != 0 }
    }
}
