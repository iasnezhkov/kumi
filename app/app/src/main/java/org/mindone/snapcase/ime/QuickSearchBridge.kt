// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import android.os.SystemClock
import androidx.compose.ui.graphics.ImageBitmap
import java.util.concurrent.ConcurrentHashMap

data class QuickSearchPending(
    val text: String = "",
    val launchTopRequested: Boolean = false,
    val dismissRequested: Boolean = false,
)

class QuickSearchHandoff(private val timeoutMs: Long = HANDOFF_TIMEOUT_MS) {
    private val buffer = StringBuilder()
    private var launchTop = false
    private var dismiss = false
    private var launchingUntilMs = 0L
    private var active = false

    @Synchronized
    fun beginLaunch(initialChar: Char?, nowMs: Long) {
        buffer.setLength(0)
        if (initialChar != null) buffer.append(initialChar)
        launchTop = false
        dismiss = false
        active = false
        launchingUntilMs = nowMs + timeoutMs
    }

    @Synchronized
    fun isHandoffWindow(nowMs: Long): Boolean = !active && nowMs <= launchingUntilMs

    @Synchronized
    fun appendChar(ch: Char) {
        if (!dismiss) buffer.append(ch)
    }

    @Synchronized
    fun backspace() {
        if (buffer.isNotEmpty()) buffer.deleteCharAt(buffer.length - 1)
    }

    @Synchronized
    fun requestLaunchTop() {
        if (!dismiss) launchTop = true
    }

    @Synchronized
    fun requestDismiss() {
        dismiss = true
        launchTop = false
        buffer.setLength(0)
        launchingUntilMs = 0L
    }

    @Synchronized
    fun setSessionActive(isActive: Boolean) {
        active = isActive
        if (isActive) launchingUntilMs = 0L
    }

    @Synchronized
    fun consumePending(): QuickSearchPending {
        val result = QuickSearchPending(
            text = buffer.toString(),
            launchTopRequested = launchTop,
            dismissRequested = dismiss,
        )
        buffer.setLength(0)
        launchTop = false
        dismiss = false
        return result
    }

    companion object {
        const val HANDOFF_TIMEOUT_MS = 800L
    }
}

object QuickSearchBridge {
    private val handoff = QuickSearchHandoff()
    private val recent = ArrayDeque<String>()
    private val icons = ConcurrentHashMap<String, ImageBitmap>()

    @Volatile
    var cachedApps: List<LaunchableApp> = emptyList()

    fun beginLaunch(initialChar: Char?) {
        handoff.beginLaunch(initialChar, SystemClock.uptimeMillis())
    }

    fun isHandoffWindow(): Boolean = handoff.isHandoffWindow(SystemClock.uptimeMillis())

    fun appendChar(ch: Char) = handoff.appendChar(ch)

    fun backspace() = handoff.backspace()

    fun requestLaunchTop() = handoff.requestLaunchTop()

    fun requestDismiss() = handoff.requestDismiss()

    fun setSessionActive(active: Boolean) = handoff.setSessionActive(active)

    fun consumePending(): QuickSearchPending = handoff.consumePending()

    @Synchronized
    fun recordLaunch(packageName: String) {
        recent.remove(packageName)
        recent.addFirst(packageName)
        while (recent.size > MAX_RECENT) recent.removeLast()
    }

    @Synchronized
    fun recentPackages(): List<String> = recent.toList()

    fun getIcon(packageName: String): ImageBitmap? = icons[packageName]

    fun putIcon(packageName: String, bitmap: ImageBitmap) {
        icons[packageName] = bitmap
    }

    private const val MAX_RECENT = 24
}
