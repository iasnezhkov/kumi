// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.system

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.input.InputManager
import android.hardware.usb.UsbConstants
import android.hardware.usb.UsbManager
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioMixerAttributes
import android.os.BatteryManager
import android.os.Handler
import android.os.Looper
import android.view.InputDevice
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import org.mindone.snapcase.transport.CaseUsb

data class PhoneBattery(
    val percent: Int?,
    val status: Int,
    val plugged: Int,
    val voltageMv: Int?,
    val temperatureDeciC: Int?,
    val currentNowUa: Int?,
)

data class SnapInputDevice(
    val id: Int,
    val name: String,
    val isExternal: Boolean,
    val keyboardType: Int,
    val languageTag: String?,
    val layoutType: String?,
    val batteryPresent: Boolean,
    val batteryCapacity: Float?,
)

data class UsbPresence(val normal: Boolean = false, val service: Boolean = false, val audioInterface: Boolean = false)

data class UsbAudioInfo(
    val id: Int,
    val productName: String,
    val isSink: Boolean,
    val isSource: Boolean,
    val sampleRates: List<Int>,
    val channelCounts: List<Int>,
    val encodings: List<String>,
    val bitPerfect: Boolean?,
)

object DeviceState {

    fun usbPresence(context: Context): UsbPresence {
        val devices = context.getSystemService(UsbManager::class.java)?.deviceList?.values ?: return UsbPresence()
        val normal = devices.firstOrNull { CaseUsb.STOCK.matches(it.vendorId, it.productId) }
        val service = devices.any { CaseUsb.SERVICE.matches(it.vendorId, it.productId) }
        val audio = normal != null && (0 until normal.interfaceCount).any { normal.getInterface(it).interfaceClass == UsbConstants.USB_CLASS_AUDIO }
        return UsbPresence(normal != null, service, audio)
    }

    fun usbManagerInfo(context: Context, vendorId: Int, productId: Int): UsbDeviceInfo? {
        val manager = context.getSystemService(UsbManager::class.java) ?: return null
        val device = manager.deviceList.values.firstOrNull { it.vendorId == vendorId && it.productId == productId } ?: return null
        val path = device.deviceName.split('/')
        val attributes = buildMap {
            device.manufacturerName?.let { put("manufacturer", it) }
            device.productName?.let { put("product", it) }
            put("bcdDevice", device.version)
            put("idVendor", "%04x".format(device.vendorId))
            put("idProduct", "%04x".format(device.productId))
            path.getOrNull(path.size - 2)?.toIntOrNull()?.let { put("busnum", it.toString()) }
            path.lastOrNull()?.toIntOrNull()?.let { put("devnum", it.toString()) }
            if (device.configurationCount > 0) {
                val configuration = device.getConfiguration(0)
                put("bMaxPower", "${configuration.maxPower}mA")
                val bits = 0x80 or (if (configuration.isSelfPowered) 0x40 else 0) or (if (configuration.isRemoteWakeup) 0x20 else 0)
                put("bmAttributes", "%02x".format(bits))
            }
        }
        return UsbDeviceInfo(device.deviceName, attributes)
    }

    fun usbAudioDevices(context: Context): Flow<List<UsbAudioInfo>> = callbackFlow {
        val manager = context.getSystemService(AudioManager::class.java)
        fun publish() {
            trySend(
                manager.getDevices(AudioManager.GET_DEVICES_INPUTS or AudioManager.GET_DEVICES_OUTPUTS)
                    .filter { it.type == AudioDeviceInfo.TYPE_USB_DEVICE || it.type == AudioDeviceInfo.TYPE_USB_HEADSET }
                    .map { describeAudio(manager, it) },
            )
        }
        val callback = object : AudioDeviceCallback() {
            override fun onAudioDevicesAdded(addedDevices: Array<out AudioDeviceInfo>?) = publish()
            override fun onAudioDevicesRemoved(removedDevices: Array<out AudioDeviceInfo>?) = publish()
        }
        manager.registerAudioDeviceCallback(callback, Handler(Looper.getMainLooper()))
        publish()
        awaitClose { manager.unregisterAudioDeviceCallback(callback) }
    }

    private fun describeAudio(manager: AudioManager, device: AudioDeviceInfo): UsbAudioInfo {
        val mixer = if (device.isSink) runCatching { manager.getSupportedMixerAttributes(device) }.getOrNull() else null
        return UsbAudioInfo(
            id = device.id,
            productName = device.productName?.toString().orEmpty(),
            isSink = device.isSink,
            isSource = device.isSource,
            sampleRates = device.sampleRates.toList().sorted(),
            channelCounts = device.channelCounts.toList().sorted(),
            encodings = device.encodings.toList().distinct().map(::encodingName),
            bitPerfect = mixer?.any { it.mixerBehavior == AudioMixerAttributes.MIXER_BEHAVIOR_BIT_PERFECT },
        )
    }

    fun encodingName(encoding: Int): String = when (encoding) {
        AudioFormat.ENCODING_PCM_16BIT -> "PCM16"
        AudioFormat.ENCODING_PCM_24BIT_PACKED -> "PCM24"
        AudioFormat.ENCODING_PCM_32BIT -> "PCM32"
        AudioFormat.ENCODING_PCM_8BIT -> "PCM8"
        AudioFormat.ENCODING_PCM_FLOAT -> "FLOAT"
        AudioFormat.ENCODING_DSD -> "DSD"
        else -> "#$encoding"
    }

    fun phoneBattery(context: Context): Flow<PhoneBattery> = callbackFlow {
        val manager = context.getSystemService(BatteryManager::class.java)
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                trySend(parseBattery(intent, manager))
            }
        }
        val sticky = context.registerReceiver(receiver, IntentFilter(Intent.ACTION_BATTERY_CHANGED), Context.RECEIVER_NOT_EXPORTED)
        if (sticky != null) trySend(parseBattery(sticky, manager))
        awaitClose { context.unregisterReceiver(receiver) }
    }

    private fun parseBattery(intent: Intent, manager: BatteryManager?): PhoneBattery {
        val level = intent.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
        val scale = intent.getIntExtra(BatteryManager.EXTRA_SCALE, -1)
        val current = manager?.getIntProperty(BatteryManager.BATTERY_PROPERTY_CURRENT_NOW)
        return PhoneBattery(
            percent = if (level >= 0 && scale > 0) level * 100 / scale else null,
            status = intent.getIntExtra(BatteryManager.EXTRA_STATUS, BatteryManager.BATTERY_STATUS_UNKNOWN),
            plugged = intent.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0),
            voltageMv = intent.getIntExtra(BatteryManager.EXTRA_VOLTAGE, -1).takeIf { it > 0 },
            temperatureDeciC = intent.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, Int.MIN_VALUE).takeIf { it != Int.MIN_VALUE },
            currentNowUa = current?.takeIf { it != Int.MIN_VALUE },
        )
    }

    fun snapInputDevices(context: Context): Flow<List<SnapInputDevice>> = callbackFlow {
        val manager = context.getSystemService(InputManager::class.java)
        fun publish() {
            trySend(manager.inputDeviceIds.toList().mapNotNull { manager.getInputDevice(it) }.filter(::isCaseKeyboard).map(::describe))
        }
        val listener = object : InputManager.InputDeviceListener {
            override fun onInputDeviceAdded(deviceId: Int) = publish()
            override fun onInputDeviceRemoved(deviceId: Int) = publish()
            override fun onInputDeviceChanged(deviceId: Int) = publish()
        }
        manager.registerInputDeviceListener(listener, Handler(Looper.getMainLooper()))
        publish()
        awaitClose { manager.unregisterInputDeviceListener(listener) }
    }

    fun isCaseKeyboard(device: InputDevice?): Boolean = device != null && CaseUsb.isKeyboard(device.vendorId, device.productId)

    private fun InputDevice.optionalString(method: String): String? = runCatching {
        InputDevice::class.java.getMethod(method).invoke(this) as? String
    }.getOrNull()

    private fun describe(device: InputDevice): SnapInputDevice {
        val battery = runCatching { device.batteryState }.getOrNull()
        return SnapInputDevice(
            id = device.id,
            name = device.name,
            isExternal = device.isExternal,
            keyboardType = device.keyboardType,
            languageTag = device.optionalString("getKeyboardLanguageTag"),
            layoutType = device.optionalString("getKeyboardLayoutType"),
            batteryPresent = battery?.isPresent == true,
            batteryCapacity = battery?.takeIf { it.isPresent }?.capacity,
        )
    }
}
