// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.system

import java.io.File
import java.nio.file.Files
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

class SysfsTest {
    private lateinit var root: File

    @Before
    fun setUp() {
        root = Files.createTempDirectory("sysfs").toFile()
    }

    @After
    fun tearDown() {
        root.deleteRecursively()
    }

    private fun put(path: String, text: String) {
        val file = File(root, path)
        file.parentFile.mkdirs()
        file.writeText(text)
    }

    @Test
    fun findsHidrawByHidId() {
        put("sys/class/hidraw/hidraw0/device/uevent", "DRIVER=hid-generic\nHID_ID=0003:000018D1:00004EE7\nHID_NAME=Other\n")
        put("sys/class/hidraw/hidraw3/device/uevent", "DRIVER=hid-generic\nHID_ID=0003:00000661:00000881\nHID_NAME=SPACETOUCH iKKO Snap\nHID_PHYS=usb-musb-hdrc.0-1/input0\n")
        val node = Sysfs(root).findHidraw(0x0661, 0x0881)
        assertEquals("hidraw3", node?.name)
        assertEquals("/dev/hidraw3", node?.devPath)
        assertEquals("SPACETOUCH iKKO Snap", node?.hidName)
        assertNull(Sysfs(root).findHidraw(0x0661, 0x0882))
    }

    @Test
    fun missingSysfsIsNotAnError() {
        assertNull(Sysfs(root).findHidraw(0x0661, 0x0881))
        assertNull(Sysfs(root).findUsbDevice(0x0661, 0x0881))
        assertTrue(Sysfs(root).typecPorts().isEmpty())
    }

    @Test
    fun findsUsbDeviceAttributes() {
        put("sys/bus/usb/devices/usb1/idVendor", "1d6b\n")
        put("sys/bus/usb/devices/usb1/idProduct", "0002\n")
        put("sys/bus/usb/devices/1-1/idVendor", "0661\n")
        put("sys/bus/usb/devices/1-1/idProduct", "0881\n")
        put("sys/bus/usb/devices/1-1/speed", "12\n")
        put("sys/bus/usb/devices/1-1/version", " 2.00\n")
        put("sys/bus/usb/devices/1-1/bmAttributes", "80\n")
        put("sys/bus/usb/devices/1-1/power/runtime_status", "active\n")
        val usb = Sysfs(root).findUsbDevice(0x0661, 0x0881)!!
        assertEquals("1-1", usb.sysfsName)
        assertEquals("12", usb.speedMbps)
        assertEquals("2.00", usb.usbVersion)
        assertEquals("active", usb.attributes["power/runtime_status"])
        assertEquals(false, Sysfs.selfPowered(usb.attributes["bmAttributes"]))
        assertEquals(true, Sysfs.selfPowered("c0"))
        assertNull(Sysfs.selfPowered(null))
    }

    @Test
    fun readsTypecRoles() {
        put("sys/class/typec/port0/power_role", "[source] sink\n")
        put("sys/class/typec/port0/data_role", "[host] device\n")
        put("sys/class/typec/port0/power_operation_mode", "default\n")
        File(root, "sys/class/typec/port0-partner").mkdirs()
        put("sys/class/typec/port1/power_role", "source [sink]\n")
        val ports = Sysfs(root).typecPorts()
        assertEquals(listOf("port0", "port1"), ports.map { it.port })
        assertEquals("source", ports[0].powerRole)
        assertEquals("host", ports[0].dataRole)
        assertEquals("default", ports[0].powerOperationMode)
        assertTrue(ports[0].partnerPresent)
        assertEquals("sink", ports[1].powerRole)
        assertFalse(ports[1].partnerPresent)
    }

    @Test
    fun parsesUevent() {
        assertEquals(mapOf("A" to "1", "B" to "x=y"), Sysfs.parseUevent("A=1\nB=x=y\nnoequals\n"))
        assertEquals("device", Sysfs.selected("host [device]"))
        assertEquals("dual", Sysfs.selected("dual\n"))
    }
}
