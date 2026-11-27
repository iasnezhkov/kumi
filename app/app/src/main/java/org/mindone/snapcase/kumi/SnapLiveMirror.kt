// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import android.content.Context
import android.net.Uri
import java.lang.ref.WeakReference
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.mindone.snapcase.transport.CaseKind

data class KumiLiveSnapshot(
    val connected: Boolean = false,
    val caseKind: String = CaseKind.NONE.name,
    val fwVersion: String? = null,
    val jack: String = Jack.Empty.name,
    val streamKind: String? = null,
    val sampleRateHz: Int = 0,
    val streamBits: Int = 0,
    val eqActiveSlot: Int = -1,
    val eqFilledMask: Int = 0,
    val crossfeed: String = CrossfeedMode.Off.name,
    val lineOut: Boolean = false,
    val stayAwake: Boolean = false,
    val hostMuted: Boolean = false,
    val activeLayer: Int = 0,
    val capsWord: Boolean = false,
    val hostLeds: Int = 0,
    val updateAvailable: Boolean = false,
    val updatedAtMs: Long = 0L,
)

object SnapLiveMirror {
    private const val PREFS_NAME = "kumi_bridge_cache"
    private const val KEY_FW = "fw_version"
    private const val KEY_JACK = "jack"
    private const val KEY_STREAM_KIND = "stream_kind"
    private const val KEY_RATE = "sample_rate_hz"
    private const val KEY_BITS = "stream_bits"
    private const val KEY_EQ_ACTIVE = "eq_active_slot"
    private const val KEY_EQ_MASK = "eq_filled_mask"
    private const val KEY_CROSSFEED = "crossfeed"
    private const val KEY_LINE_OUT = "line_out"
    private const val KEY_STAY_AWAKE = "stay_awake"
    private const val KEY_HOST_MUTED = "host_muted"
    private const val KEY_LAYER = "active_layer"
    private const val KEY_CAPS_WORD = "caps_word"
    private const val KEY_UPDATE = "update_available"
    private const val KEY_UPDATED_AT = "updated_at_ms"
    private const val KEY_CONTROLS_ALLOWED = "controls_allowed"

    private val _snapshot = MutableStateFlow(KumiLiveSnapshot())
    val snapshot: StateFlow<KumiLiveSnapshot> = _snapshot.asStateFlow()

    @Volatile private var appContext: Context? = null
    @Volatile private var sessionRef: WeakReference<KumiSession>? = null
    @Volatile private var updatePending: Boolean = false
    @Volatile var hardwareMediaGesturesActive: Boolean = false
        private set

    fun bindContext(context: Context) {
        appContext = context.applicationContext
    }

    fun bindSession(session: KumiSession?) {
        sessionRef = session?.let { WeakReference(it) }
    }

    fun activeSession(): KumiSession? = sessionRef?.get()

    fun setUpdateAvailable(available: Boolean) {
        if (updatePending == available) return
        updatePending = available
        _snapshot.value = _snapshot.value.copy(updateAvailable = available)
        appContext?.let { ctx ->
            ctx.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE).edit().putBoolean(KEY_UPDATE, available).apply()
            notifyObserver(ctx)
        }
    }

    fun setControlsAllowed(context: Context, allowed: Boolean) {
        val ctx = context.applicationContext
        ctx.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE).edit().putBoolean(KEY_CONTROLS_ALLOWED, allowed).apply()
        notifyObserver(ctx)
    }

    fun controlsAllowed(context: Context): Boolean =
        context.applicationContext.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE).getBoolean(KEY_CONTROLS_ALLOWED, false)

    fun update(state: KumiState, kind: CaseKind = if (state.connected) CaseKind.KUMI else CaseKind.NONE) {
        hardwareMediaGesturesActive = state.connected && state.mediaGestures?.enabled == true
        val kb = state.keyboard
        val audio = state.audio
        val stream = audio?.stream
        var mask = 0
        state.eqSlots.forEachIndexed { idx, profile ->
            if (profile != null && profile.bands.isNotEmpty()) mask = mask or (1 shl idx)
        }
        val next = KumiLiveSnapshot(
            connected = state.connected,
            caseKind = kind.name,
            fwVersion = state.device?.firmware?.toString() ?: _snapshot.value.fwVersion,
            jack = audio?.jack?.name ?: Jack.Empty.name,
            streamKind = stream?.kind?.name,
            sampleRateHz = stream?.rateHz ?: 0,
            streamBits = stream?.bits ?: 0,
            eqActiveSlot = audio?.eqActive ?: -1,
            eqFilledMask = mask,
            crossfeed = state.dsp?.crossfeed?.name ?: CrossfeedMode.Off.name,
            lineOut = state.dacExtras?.lineOut == true,
            stayAwake = state.dacExtras?.stayAwakeWhileActive == true,
            hostMuted = audio?.hostMuted == true,
            activeLayer = kb?.activeLayer ?: 0,
            capsWord = kb?.capsWord == true,
            hostLeds = kb?.hostLeds ?: 0,
            updateAvailable = updatePending,
            updatedAtMs = System.currentTimeMillis(),
        )
        publish(next)
    }

    fun publishFromBridge(next: KumiLiveSnapshot) {
        publish(next)
    }

    fun clear() {
        hardwareMediaGesturesActive = false
        val prev = _snapshot.value
        _snapshot.value = KumiLiveSnapshot(
            fwVersion = prev.fwVersion,
            eqFilledMask = prev.eqFilledMask,
            updateAvailable = updatePending,
            updatedAtMs = System.currentTimeMillis(),
        )
        appContext?.let(::notifyObserver)
    }

    fun persisted(context: Context): KumiLiveSnapshot {
        val live = _snapshot.value
        if (live.connected) return live
        val p = context.applicationContext.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        val updated = p.getLong(KEY_UPDATED_AT, 0L)
        if (updated == 0L) return live.copy(updateAvailable = p.getBoolean(KEY_UPDATE, updatePending))
        return KumiLiveSnapshot(
            connected = false,
            caseKind = live.caseKind,
            fwVersion = p.getString(KEY_FW, null),
            jack = p.getString(KEY_JACK, Jack.Empty.name) ?: Jack.Empty.name,
            streamKind = p.getString(KEY_STREAM_KIND, null),
            sampleRateHz = p.getInt(KEY_RATE, 0),
            streamBits = p.getInt(KEY_BITS, 0),
            eqActiveSlot = p.getInt(KEY_EQ_ACTIVE, -1),
            eqFilledMask = p.getInt(KEY_EQ_MASK, 0),
            crossfeed = p.getString(KEY_CROSSFEED, CrossfeedMode.Off.name) ?: CrossfeedMode.Off.name,
            lineOut = p.getBoolean(KEY_LINE_OUT, false),
            stayAwake = p.getBoolean(KEY_STAY_AWAKE, false),
            hostMuted = p.getBoolean(KEY_HOST_MUTED, false),
            activeLayer = p.getInt(KEY_LAYER, 0),
            capsWord = p.getBoolean(KEY_CAPS_WORD, false),
            updateAvailable = p.getBoolean(KEY_UPDATE, updatePending),
            updatedAtMs = updated,
        )
    }

    private fun publish(next: KumiLiveSnapshot) {
        _snapshot.value = next
        val ctx = appContext ?: return
        if (next.connected || next.updatedAtMs > 0L) {
            ctx.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE).edit()
                .putString(KEY_FW, next.fwVersion)
                .putString(KEY_JACK, next.jack)
                .putString(KEY_STREAM_KIND, next.streamKind)
                .putInt(KEY_RATE, next.sampleRateHz)
                .putInt(KEY_BITS, next.streamBits)
                .putInt(KEY_EQ_ACTIVE, next.eqActiveSlot)
                .putInt(KEY_EQ_MASK, next.eqFilledMask)
                .putString(KEY_CROSSFEED, next.crossfeed)
                .putBoolean(KEY_LINE_OUT, next.lineOut)
                .putBoolean(KEY_STAY_AWAKE, next.stayAwake)
                .putBoolean(KEY_HOST_MUTED, next.hostMuted)
                .putInt(KEY_LAYER, next.activeLayer)
                .putBoolean(KEY_CAPS_WORD, next.capsWord)
                .putBoolean(KEY_UPDATE, next.updateAvailable)
                .putLong(KEY_UPDATED_AT, next.updatedAtMs)
                .apply()
        }
        notifyObserver(ctx)
    }

    fun notifyObserver(context: Context) {
        runCatching {
            val uri = Uri.parse("content://${context.packageName}.bridge/state")
            context.contentResolver.notifyChange(uri, null)
        }
    }
}
