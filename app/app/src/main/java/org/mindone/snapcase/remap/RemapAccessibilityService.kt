// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.remap

import android.content.Intent
import android.content.pm.PackageManager
import android.accessibilityservice.AccessibilityService
import android.hardware.input.InputManager
import android.os.SystemClock
import android.util.Log
import android.view.InputDevice
import android.view.InputEvent
import android.view.KeyCharacterMap
import android.view.KeyEvent
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import java.lang.reflect.Method
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import org.mindone.snapcase.data.SettingsRepository
import org.mindone.snapcase.ime.QuickLauncher
import org.mindone.snapcase.ime.QuickSearchActivity
import org.mindone.snapcase.ime.QuickSearchBridge
import org.mindone.snapcase.ime.SpeedKeyBinding
import org.mindone.snapcase.system.DeviceState

class KeyInjector(private val inputManager: InputManager) {
    private val method: Method? = runCatching {
        InputManager::class.java.getMethod("injectInputEvent", InputEvent::class.java, Int::class.javaPrimitiveType)
    }.getOrNull()

    val available: Boolean get() = method != null

    fun inject(output: KeyOutput, downTime: Long): InjectResult {
        val target = method ?: return InjectResult.TRANSIENT_FAILURE
        val now = SystemClock.uptimeMillis()
        val event = KeyEvent(
            downTime, now,
            if (output.down) KeyEvent.ACTION_DOWN else KeyEvent.ACTION_UP,
            output.keyCode, output.repeatCount, output.metaState,
            KeyCharacterMap.VIRTUAL_KEYBOARD, 0, KeyEvent.FLAG_FROM_SYSTEM, InputDevice.SOURCE_KEYBOARD,
        )
        return try {
            if (target.invoke(inputManager, event, INJECT_MODE_ASYNC) as? Boolean == true) InjectResult.OK else InjectResult.TRANSIENT_FAILURE
        } catch (error: java.lang.reflect.InvocationTargetException) {
            Log.w(TAG, "Key injection failed", error.targetException)
            if (error.targetException is SecurityException) InjectResult.PERMISSION_DENIED else InjectResult.TRANSIENT_FAILURE
        } catch (error: SecurityException) {
            Log.w(TAG, "Key injection denied", error)
            InjectResult.PERMISSION_DENIED
        } catch (error: Exception) {
            Log.w(TAG, "Key injection failed", error)
            InjectResult.TRANSIENT_FAILURE
        }
    }

    enum class InjectResult { OK, TRANSIENT_FAILURE, PERMISSION_DENIED }

    companion object {
        private const val INJECT_MODE_ASYNC = 0
        private const val TAG = "SnapRemap"
    }
}

class RemapAccessibilityService : AccessibilityService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val engine = RemapEngine()
    private lateinit var injector: KeyInjector
    private lateinit var inputManager: InputManager

    @Volatile
    private var onlySnap = true
    @Volatile
    private var speedKeysEnabled = true
    @Volatile
    private var speedKeys: List<SpeedKeyBinding> = emptyList()
    @Volatile
    private var homeTypeToSearch = true
    private val downTimes = HashMap<Int, Long>()
    private val speedConsumedKeys = HashSet<Int>()
    private val quickSearchConsumedKeys = HashSet<Int>()
    private var injectionFailed = false

    override fun onServiceConnected() {
        inputManager = getSystemService(InputManager::class.java)
        injector = KeyInjector(inputManager)
        injectionFailed = false
        active = true
        scope.launch(Dispatchers.IO) {
            if (QuickSearchBridge.cachedApps.isEmpty()) {
                QuickSearchBridge.cachedApps = QuickLauncher.listLaunchableApps(this@RemapAccessibilityService)
            }
        }
        scope.launch {
            SettingsRepository.from(this@RemapAccessibilityService).settings.collectLatest {
                engine.table = it.remapTable
                onlySnap = it.remapOnlySnap
                speedKeysEnabled = it.speedKeysEnabled
                speedKeys = it.speedKeys
                homeTypeToSearch = it.homeTypeToSearch
            }
        }
    }

    private fun tryQuickSearchHandoff(event: KeyEvent): Boolean {
        if (event.action == KeyEvent.ACTION_UP) {
            return quickSearchConsumedKeys.remove(event.keyCode)
        }
        if (event.action == KeyEvent.ACTION_DOWN && event.repeatCount > 0 && event.keyCode in quickSearchConsumedKeys) {
            return true
        }
        if (!QuickSearchBridge.isHandoffWindow()) return false
        val rootPkg = rootInActiveWindow?.packageName?.toString()
        if (rootPkg == packageName) return false
        if (event.action != KeyEvent.ACTION_DOWN || event.repeatCount != 0) return false
        if (event.isCtrlPressed || event.isAltPressed || event.isMetaPressed) return false
        when (event.keyCode) {
            KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> {
                QuickSearchBridge.requestLaunchTop()
                quickSearchConsumedKeys.add(event.keyCode)
                return true
            }
            KeyEvent.KEYCODE_DEL -> {
                QuickSearchBridge.backspace()
                quickSearchConsumedKeys.add(event.keyCode)
                return true
            }
            KeyEvent.KEYCODE_ESCAPE -> {
                QuickSearchBridge.requestDismiss()
                quickSearchConsumedKeys.add(event.keyCode)
                return true
            }
        }
        val ch = QuickLauncher.extractSearchChar(event.keyCode, event.unicodeChar, event.isShiftPressed) ?: return false
        QuickSearchBridge.appendChar(ch)
        quickSearchConsumedKeys.add(event.keyCode)
        return true
    }

    private fun tryHomeTypeToSearch(event: KeyEvent): Boolean {
        if (!homeTypeToSearch) return false
        if (event.action != KeyEvent.ACTION_DOWN || event.repeatCount != 0) return false
        if (event.isCtrlPressed || event.isAltPressed || event.isMetaPressed) return false
        val ch = QuickLauncher.extractSearchChar(event.keyCode, event.unicodeChar, event.isShiftPressed) ?: return false
        if (!isOnLauncherWithoutInputFocus()) return false
        val launched = QuickSearchActivity.launch(this, ch)
        if (launched) quickSearchConsumedKeys.add(event.keyCode)
        return launched
    }

    private fun trySpeedKey(event: KeyEvent): Boolean {
        if (event.action == KeyEvent.ACTION_UP) {
            return speedConsumedKeys.remove(event.keyCode)
        }
        if (event.action == KeyEvent.ACTION_DOWN && event.repeatCount > 0 && event.keyCode in speedConsumedKeys) {
            return true
        }
        if (!speedKeysEnabled || speedKeys.isEmpty()) return false
        if (event.action != KeyEvent.ACTION_DOWN || event.repeatCount != 0) return false
        val binding = QuickLauncher.match(event.keyCode, speedKeys) ?: return false
        val metaTrigger = event.isMetaPressed && !event.isCtrlPressed && !event.isAltPressed
        val homeTrigger = !event.isMetaPressed && !event.isCtrlPressed && !event.isAltPressed && !event.isShiftPressed && isOnLauncherWithoutInputFocus()
        if (!metaTrigger && !homeTrigger) return false
        val launched = QuickLauncher.launch(this, binding.packageName)
        if (launched) speedConsumedKeys.add(event.keyCode)
        return launched
    }

    private fun isOnLauncherWithoutInputFocus(): Boolean {
        val focused = findFocus(AccessibilityNodeInfo.FOCUS_INPUT)
        if (focused != null && focused.isEditable) return false
        val rootPkg = rootInActiveWindow?.packageName?.toString() ?: return false
        val homeIntent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME)
        val defaultPkg = packageManager.resolveActivity(homeIntent, PackageManager.MATCH_DEFAULT_ONLY)?.activityInfo?.packageName
        if (!defaultPkg.isNullOrBlank() && defaultPkg != "android") {
            return rootPkg == defaultPkg
        }
        val installedHomes = packageManager.queryIntentActivities(homeIntent, 0)
            .mapNotNull { it.activityInfo?.packageName }
            .toSet()
        return QuickLauncher.isHomeLauncherPackage(rootPkg, defaultPkg, installedHomes)
    }

    override fun onKeyEvent(event: KeyEvent): Boolean {
        if (event.deviceId != KeyCharacterMap.VIRTUAL_KEYBOARD) {
            if (tryQuickSearchHandoff(event)) return true
            if (trySpeedKey(event)) return true
            if (tryHomeTypeToSearch(event)) return true
        }
        if (!::injector.isInitialized || injectionFailed ||
            event.deviceId == KeyCharacterMap.VIRTUAL_KEYBOARD || !injector.available ||
            checkSelfPermission(INJECT_EVENTS) != PackageManager.PERMISSION_GRANTED
        ) return false
        if (event.action != KeyEvent.ACTION_DOWN && event.action != KeyEvent.ACTION_UP) return false
        val device = inputManager.getInputDevice(event.deviceId)
        val fromTarget = if (onlySnap) DeviceState.isCaseKeyboard(device) else device != null && !device.isVirtual
        val decision = engine.process(
            KeyInput(event.keyCode, event.action == KeyEvent.ACTION_DOWN, event.metaState, event.repeatCount, fromTarget),
        )
        return when (decision) {
            RemapDecision.PassThrough -> false
            is RemapDecision.Replace -> {
                decision.events.forEach { output ->
                    val downTime = if (output.down && output.repeatCount == 0) {
                        SystemClock.uptimeMillis().also { downTimes[output.keyCode] = it }
                    } else {
                        downTimes[output.keyCode] ?: event.downTime
                    }
                    when (injector.inject(output, downTime)) {
                        KeyInjector.InjectResult.OK -> Unit
                        KeyInjector.InjectResult.PERMISSION_DENIED -> {
                            releaseKeys()
                            injectionFailed = true
                            return false
                        }
                        KeyInjector.InjectResult.TRANSIENT_FAILURE -> {
                            releaseKeys()
                            return false
                        }
                    }
                    if (!output.down) downTimes.remove(output.keyCode)
                }
                true
            }
        }
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {}

    private fun releaseKeys() {
        if (::injector.isInitialized) {
            downTimes.toMap().forEach { (keyCode, downTime) ->
                injector.inject(KeyOutput(keyCode, false, 0, 0), downTime)
            }
        }
        downTimes.clear()
        engine.reset()
    }

    override fun onInterrupt() {
        releaseKeys()
    }

    override fun onDestroy() {
        releaseKeys()
        active = false
        scope.cancel()
        super.onDestroy()
    }

    companion object {
        const val INJECT_EVENTS = "android.permission.INJECT_EVENTS"

        @Volatile
        var active = false
            private set
    }
}
