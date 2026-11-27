// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.bridge

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.content.pm.PackageManager
import android.database.Cursor
import android.database.MatrixCursor
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbManager
import android.net.Uri
import android.os.Bundle
import org.mindone.snapcase.kumi.CrossfeedMode
import org.mindone.snapcase.kumi.EqProfile
import org.mindone.snapcase.kumi.KumiClient
import org.mindone.snapcase.kumi.KumiLiveSnapshot
import org.mindone.snapcase.kumi.KumiNative
import org.mindone.snapcase.kumi.KumiUsb
import org.mindone.snapcase.kumi.NativeBridge
import org.mindone.snapcase.kumi.SnapLiveMirror
import org.mindone.snapcase.kumi.UsbKumiLink
import org.mindone.snapcase.transport.CaseKind
import org.mindone.snapcase.transport.CaseUsb

class KumiBridgeProvider : ContentProvider() {

    override fun onCreate(): Boolean {
        context?.let(SnapLiveMirror::bindContext)
        return true
    }

    private fun verifyCaller(context: Context): Boolean {
        val caller = callingPackage ?: return false
        if (caller == context.packageName) return true
        return context.packageManager.checkSignatures(context.packageName, caller) == PackageManager.SIGNATURE_MATCH
    }

    override fun query(
        uri: Uri,
        projection: Array<out String>?,
        selection: String?,
        selectionArgs: Array<out String>?,
        sortOrder: String?,
    ): Cursor? {
        val ctx = context ?: return null
        if (!verifyCaller(ctx)) return null
        if (uri.lastPathSegment != PATH_STATE) return null
        val liveKind = detectCaseKind(ctx)
        val snap = SnapLiveMirror.persisted(ctx)
        val resolvedKind = if (liveKind != CaseKind.NONE) liveKind.name else snap.caseKind
        val controlsAllowed = if (SnapLiveMirror.controlsAllowed(ctx)) 1 else 0
        val cursor = MatrixCursor(COLUMNS, 1)
        cursor.addRow(
            arrayOf<Any?>(
                PROTOCOL_VERSION,
                resolvedKind,
                if (snap.connected) 1 else 0,
                snap.fwVersion,
                snap.jack,
                snap.streamKind,
                snap.sampleRateHz,
                snap.streamBits,
                snap.eqActiveSlot,
                snap.eqFilledMask,
                snap.crossfeed,
                if (snap.lineOut) 1 else 0,
                if (snap.stayAwake) 1 else 0,
                if (snap.hostMuted) 1 else 0,
                snap.activeLayer,
                if (snap.capsWord) 1 else 0,
                if (snap.updateAvailable) 1 else 0,
                controlsAllowed,
                snap.updatedAtMs,
            ),
        )
        return cursor
    }

    override fun call(method: String, arg: String?, extras: Bundle?): Bundle {
        val ctx = context ?: return resultBundle(false, "no_context")
        if (!verifyCaller(ctx)) {
            return resultBundle(false, "unauthorized")
        }
        if (!SnapLiveMirror.controlsAllowed(ctx)) {
            return resultBundle(false, "disabled_by_user")
        }
        val value = extras?.getInt(EXTRA_VALUE, -1) ?: -1
        val liveSession = SnapLiveMirror.activeSession()
        if (liveSession != null && liveSession.isLiveConnected) {
            val ok = liveSession.enqueueBridgeCommand(method, value)
            return resultBundle(ok, if (ok) null else "unsupported_command")
        }
        return executeOneShot(ctx, method, value)
    }

    private fun executeOneShot(context: Context, command: String, value: Int): Bundle = synchronized(ONE_SHOT_LOCK) {
        val manager = context.getSystemService(UsbManager::class.java) ?: return resultBundle(false, "no_usb")
        val device = findKumiDevice(manager) ?: return resultBundle(false, "no_case")
        if (!manager.hasPermission(device)) return resultBundle(false, "no_usb_permission")
        if (!KumiNative.loaded) return resultBundle(false, "no_native_bridge")
        val link = runCatching { UsbKumiLink.open(manager, device) }.getOrNull()
            ?: return resultBundle(false, "open_failed")
        return try {
            val client = KumiClient(NativeBridge, link)
            val prev = SnapLiveMirror.persisted(context)
            when (command) {
                CMD_TOGGLE_EQ -> {
                    val audio = client.audioStatus()
                    val target = if (audio.eqActive != null) null else firstFilledSlot(prev.eqFilledMask)
                    client.selectEq(target)
                }
                CMD_SELECT_EQ -> {
                    val slot = value.takeIf { it in 0 until EqProfile.SLOTS }
                    client.selectEq(slot)
                }
                CMD_CYCLE_CROSSFEED -> {
                    val dsp = client.dsp()
                    val modes = CrossfeedMode.entries
                    val next = modes[(modes.indexOf(dsp.crossfeed) + 1) % modes.size]
                    client.setDsp(dsp.copy(crossfeed = next))
                }
                CMD_TOGGLE_STAY_AWAKE -> {
                    val extras = client.dacExtras()
                    client.setDacExtras(extras.copy(stayAwakeWhileActive = !extras.stayAwakeWhileActive))
                }
                CMD_REFRESH -> Unit
                else -> return resultBundle(false, "unsupported_command")
            }
            val deviceInfo = runCatching { client.deviceInfo() }.getOrNull()
            val audio = runCatching { client.audioStatus() }.getOrNull()
            val dsp = runCatching { client.dsp() }.getOrNull()
            val dacExtras = runCatching { client.dacExtras() }.getOrNull()
            val kb = runCatching { client.keyboardStatus() }.getOrNull()
            val stream = audio?.stream
            val snap = KumiLiveSnapshot(
                connected = false,
                caseKind = CaseKind.KUMI.name,
                fwVersion = deviceInfo?.firmware?.toString() ?: prev.fwVersion,
                jack = audio?.jack?.name ?: prev.jack,
                streamKind = stream?.kind?.name,
                sampleRateHz = stream?.rateHz ?: 0,
                streamBits = stream?.bits ?: 0,
                eqActiveSlot = audio?.eqActive ?: prev.eqActiveSlot,
                eqFilledMask = prev.eqFilledMask,
                crossfeed = dsp?.crossfeed?.name ?: prev.crossfeed,
                lineOut = dacExtras?.lineOut ?: prev.lineOut,
                stayAwake = dacExtras?.stayAwakeWhileActive ?: prev.stayAwake,
                hostMuted = audio?.hostMuted ?: prev.hostMuted,
                activeLayer = kb?.activeLayer ?: prev.activeLayer,
                capsWord = kb?.capsWord ?: prev.capsWord,
                updateAvailable = prev.updateAvailable,
                updatedAtMs = System.currentTimeMillis(),
            )
            SnapLiveMirror.publishFromBridge(snap)
            resultBundle(true, null)
        } catch (error: Exception) {
            resultBundle(false, error.message ?: "rpc_error")
        } finally {
            runCatching { link.close() }
            runCatching { NativeBridge.resetSession() }
        }
    }

    private fun detectCaseKind(context: Context): CaseKind {
        val manager = context.getSystemService(UsbManager::class.java) ?: return CaseKind.NONE
        val kinds = manager.deviceList.values.map { device ->
            when (val kind = CaseUsb.kindOf(device.vendorId, device.productId)) {
                CaseKind.KUMI -> if (KumiUsb.controlInterface(device) != null) kind else CaseKind.NONE
                else -> kind
            }
        }
        return CaseUsb.strongest(kinds)
    }

    private fun findKumiDevice(manager: UsbManager): UsbDevice? =
        manager.deviceList.values.firstOrNull { KumiUsb.isKumi(it) }

    private fun firstFilledSlot(mask: Int): Int {
        for (i in 0 until EqProfile.SLOTS) {
            if ((mask and (1 shl i)) != 0) return i
        }
        return 0
    }

    private fun resultBundle(ok: Boolean, error: String?): Bundle = Bundle().apply {
        putBoolean(KEY_OK, ok)
        if (error != null) putString(KEY_ERROR, error)
    }

    override fun getType(uri: Uri): String? =
        if (uri.lastPathSegment == PATH_STATE) "vnd.android.cursor.item/vnd.org.mindone.snapcase.state" else null

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int = 0

    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?): Int = 0

    companion object {
        const val PROTOCOL_VERSION = 1
        const val PATH_STATE = "state"
        const val EXTRA_VALUE = "value"
        const val KEY_OK = "ok"
        const val KEY_ERROR = "error"

        const val CMD_TOGGLE_EQ = "toggle_eq"
        const val CMD_SELECT_EQ = "select_eq"
        const val CMD_CYCLE_CROSSFEED = "cycle_crossfeed"
        const val CMD_TOGGLE_STAY_AWAKE = "toggle_stay_awake"
        const val CMD_REFRESH = "refresh"

        const val ACTION_OPEN_SCREEN = "org.mindone.snapcase.action.OPEN_SCREEN"
        const val EXTRA_SCREEN = "screen"
        const val EXTRA_EQ_SLOT = "eq_slot"

        private val ONE_SHOT_LOCK = Any()

        private val COLUMNS = arrayOf(
            "protocol_version",
            "case_kind",
            "connected",
            "fw_version",
            "jack",
            "stream_kind",
            "sample_rate_hz",
            "stream_bits",
            "eq_active_slot",
            "eq_filled_mask",
            "crossfeed",
            "line_out",
            "stay_awake",
            "host_muted",
            "active_layer",
            "caps_word",
            "update_available",
            "controls_allowed",
            "updated_at_ms",
        )
    }
}
