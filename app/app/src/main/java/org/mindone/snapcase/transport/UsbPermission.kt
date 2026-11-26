// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.transport

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbManager
import kotlin.coroutines.resume
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeoutOrNull

object UsbPermission {
    private const val ACTION = "org.mindone.snapcase.USB_PERMISSION"
    private const val WAIT_MS = 60_000L

    suspend fun request(context: Context, manager: UsbManager, device: UsbDevice): Boolean {
        if (manager.hasPermission(device)) return true
        val granted = withTimeoutOrNull(WAIT_MS) {
            suspendCancellableCoroutine { continuation ->
                val receiver = object : BroadcastReceiver() {
                    override fun onReceive(context: Context, intent: Intent) {
                        val target = intent.getParcelableExtra(UsbManager.EXTRA_DEVICE, UsbDevice::class.java)
                        if (target != null && target.deviceId != device.deviceId) return
                        runCatching { context.unregisterReceiver(this) }
                        if (continuation.isActive) continuation.resume(intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false))
                    }
                }
                context.registerReceiver(receiver, IntentFilter(ACTION), Context.RECEIVER_NOT_EXPORTED)
                continuation.invokeOnCancellation { runCatching { context.unregisterReceiver(receiver) } }
                val intent = Intent(ACTION).setPackage(context.packageName)
                val pending = PendingIntent.getBroadcast(
                    context,
                    device.deviceId,
                    intent,
                    PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                )
                manager.requestPermission(device, pending)
            }
        }
        return granted == true
    }
}
