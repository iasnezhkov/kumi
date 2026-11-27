// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.remap

import android.accessibilityservice.AccessibilityService
import android.app.KeyguardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.hardware.input.InputManager
import android.media.AudioManager
import android.os.SystemClock
import android.text.InputType
import android.util.Log
import android.view.InputDevice
import android.view.InputEvent
import android.view.KeyCharacterMap
import android.view.KeyEvent
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityWindowInfo
import java.lang.reflect.Method
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import org.mindone.snapcase.data.SettingsRepository
import org.mindone.snapcase.ime.QuickLauncher
import org.mindone.snapcase.ime.QuickSearchActivity
import org.mindone.snapcase.ime.QuickSearchBridge
import org.mindone.snapcase.ime.QuickSearchOverlayController
import org.mindone.snapcase.ime.SnapStripImeService
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
    private var keyguardManager: KeyguardManager? = null
    private var audioManager: AudioManager? = null
    private var overlayController: QuickSearchOverlayController? = null

    @Volatile
    private var onlySnap = true
    @Volatile
    private var speedKeysEnabled = true
    @Volatile
    private var speedKeys: List<SpeedKeyBinding> = emptyList()
    @Volatile
    private var homeTypeToSearch = true
    @Volatile
    private var backspaceAsBack = false
    @Volatile
    private var mediaDoubleTap = false
    @Volatile
    private var backspaceExcluded: Set<String> = SettingsRepository.DEFAULT_BACKSPACE_EXCLUDED

    private var cachedDefaultHome: String? = null
    private var cachedInstalledHomes: Set<String> = emptySet()
    private var cachedNativeKeyHomes: Set<String> = emptySet()
    private var homeCacheUpdatedAt = 0L
    private var lastAppWindowPackage: String? = null
    private var lastMediaTapKey: Int = KeyEvent.KEYCODE_UNKNOWN
    private var lastMediaTapMs: Long = 0L
    private var shiftUsedWithOtherKey: Boolean = false
    private var pendingBackJob: Job? = null

    private val downTimes = HashMap<Int, Long>()
    private val speedConsumedKeys = HashSet<Int>()
    private val quickSearchConsumedKeys = HashSet<Int>()
    private val backConsumedKeys = HashSet<Int>()
    private val mediaConsumedKeys = HashSet<Int>()
    private var injectionFailed = false

    override fun onServiceConnected() {
        inputManager = getSystemService(InputManager::class.java)
        keyguardManager = getSystemService(KeyguardManager::class.java)
        audioManager = getSystemService(AudioManager::class.java)
        injector = KeyInjector(inputManager)
        overlayController = QuickSearchOverlayController(this, scope)
        injectionFailed = false
        setServiceActive(true)
        refreshHomeLauncherCache(force = true)
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
                backspaceAsBack = it.backspaceAsBack
                mediaDoubleTap = it.mediaDoubleTap
                backspaceExcluded = it.backspaceBackExcluded
                QuickSearchBridge.seedRecent(it.quickSearchRecent)
            }
        }
    }

    private fun refreshHomeLauncherCache(force: Boolean = false) {
        val now = SystemClock.uptimeMillis()
        if (!force && now - homeCacheUpdatedAt < HOME_CACHE_TTL_MS) return
        val homeIntent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME)
        cachedDefaultHome = packageManager.resolveActivity(homeIntent, PackageManager.MATCH_DEFAULT_ONLY)?.activityInfo?.packageName
        val homeActivities = packageManager.queryIntentActivities(homeIntent, PackageManager.GET_META_DATA)
        cachedInstalledHomes = homeActivities.mapNotNull { it.activityInfo?.packageName }.toSet()
        cachedNativeKeyHomes = homeActivities.mapNotNull { info ->
            val ai = info.activityInfo ?: return@mapNotNull null
            if (ai.metaData?.getBoolean(META_NATIVE_HOME_KEYS, false) == true) ai.packageName else null
        }.toSet()
        homeCacheUpdatedAt = now
        _detectedHomeFlow.value = cachedDefaultHome?.takeIf { it != "android" } ?: cachedInstalledHomes.firstOrNull { it != "android" }
    }

    private fun resolveForegroundAppPackage(): String? {
        val interactiveWindows = runCatching { windows }.getOrNull()
        if (!interactiveWindows.isNullOrEmpty()) {
            val activeApp = interactiveWindows
                .filter { it.type == AccessibilityWindowInfo.TYPE_APPLICATION }
                .sortedByDescending { (if (it.isFocused) 4 else 0) + (if (it.isActive) 2 else 0) }
                .firstOrNull { w ->
                    val pkg = w.root?.packageName?.toString()
                    !pkg.isNullOrBlank() && pkg != "com.android.systemui"
                }
            val pkg = activeApp?.root?.packageName?.toString()
            if (!pkg.isNullOrBlank()) {
                lastAppWindowPackage = pkg
                return pkg
            }
        }
        val rootPkg = rootInActiveWindow?.packageName?.toString()
        if (!rootPkg.isNullOrBlank() && rootPkg != "com.android.systemui") {
            lastAppWindowPackage = rootPkg
            return rootPkg
        }
        return lastAppWindowPackage
    }

    private fun isKeyguardLocked(): Boolean = keyguardManager?.isKeyguardLocked == true

    private fun isEditableInputFocused(): Boolean {
        if (SnapStripImeService.editableInputActive) return true
        val focused = findFocus(AccessibilityNodeInfo.FOCUS_INPUT)
        if (focused != null && isNodeEditable(focused)) return true
        val interactiveWindows = runCatching { windows }.getOrNull()
        if (interactiveWindows != null) {
            for (win in interactiveWindows) {
                if (win.type == AccessibilityWindowInfo.TYPE_INPUT_METHOD) {
                    val bounds = android.graphics.Rect()
                    win.getBoundsInScreen(bounds)
                    val minSoftKeyboardPx = (90f * resources.displayMetrics.density).toInt()
                    if (bounds.height() > minSoftKeyboardPx) return true
                }
            }
        }
        return false
    }

    private fun isOnHomeLauncher(fgPkg: String?): Boolean {
        refreshHomeLauncherCache()
        return QuickLauncher.isHomeLauncherPackage(fgPkg, cachedDefaultHome, cachedInstalledHomes)
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

    private fun tryHomeTypeToSearch(event: KeyEvent, onHomeClean: Boolean): Boolean {
        if (!homeTypeToSearch || !onHomeClean) return false
        if (event.action != KeyEvent.ACTION_DOWN || event.repeatCount != 0) return false
        if (event.isCtrlPressed || event.isAltPressed || event.isMetaPressed) return false
        val ch = QuickLauncher.extractSearchChar(event.keyCode, event.unicodeChar, event.isShiftPressed) ?: return false
        val controller = overlayController
        if (controller != null && controller.show(ch)) {
            quickSearchConsumedKeys.add(event.keyCode)
            return true
        }
        val launched = QuickSearchActivity.launch(this, ch)
        if (launched) quickSearchConsumedKeys.add(event.keyCode)
        return launched
    }

    private fun trySpeedKey(event: KeyEvent, onHomeClean: Boolean): Boolean {
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
        val homeTrigger = !event.isMetaPressed && !event.isCtrlPressed && !event.isAltPressed && !event.isShiftPressed && onHomeClean
        if (!metaTrigger && !homeTrigger) return false
        val launched = QuickLauncher.launch(this, binding.packageName)
        if (launched) speedConsumedKeys.add(event.keyCode)
        return launched
    }

    private fun dispatchMediaKey(mediaKeyCode: Int): Boolean {
        val am = audioManager ?: return false
        val now = SystemClock.uptimeMillis()
        am.dispatchMediaKeyEvent(KeyEvent(now, now, KeyEvent.ACTION_DOWN, mediaKeyCode, 0))
        am.dispatchMediaKeyEvent(KeyEvent(now, now, KeyEvent.ACTION_UP, mediaKeyCode, 0))
        return true
    }

    private fun tryMediaDoubleTap(event: KeyEvent, editable: Boolean): Boolean {
        if (!mediaDoubleTap) return false
        val code = event.keyCode
        if (event.action == KeyEvent.ACTION_UP) {
            if (code == KeyEvent.KEYCODE_SHIFT_LEFT && shiftUsedWithOtherKey) {
                lastMediaTapKey = KeyEvent.KEYCODE_UNKNOWN
                lastMediaTapMs = 0L
                shiftUsedWithOtherKey = false
            }
            return mediaConsumedKeys.remove(code)
        }
        if (event.action != KeyEvent.ACTION_DOWN) return false
        if (code != KeyEvent.KEYCODE_SHIFT_LEFT && event.isShiftPressed) {
            shiftUsedWithOtherKey = true
        }
        if (editable) {
            lastMediaTapKey = KeyEvent.KEYCODE_UNKNOWN
            lastMediaTapMs = 0L
            return false
        }
        if (event.repeatCount > 0) {
            return code in mediaConsumedKeys
        }
        val mediaKey = when (code) {
            KeyEvent.KEYCODE_SHIFT_LEFT -> if (!event.isCtrlPressed && !event.isAltPressed && !event.isMetaPressed) KeyEvent.KEYCODE_MEDIA_PREVIOUS else 0
            KeyEvent.KEYCODE_DEL -> if (!event.isCtrlPressed && !event.isAltPressed && !event.isMetaPressed && !event.isShiftPressed) KeyEvent.KEYCODE_MEDIA_NEXT else 0
            KeyEvent.KEYCODE_SPACE -> if (!event.isCtrlPressed && !event.isAltPressed && !event.isMetaPressed && !event.isShiftPressed) KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE else 0
            else -> 0
        }
        if (mediaKey == 0) {
            lastMediaTapKey = KeyEvent.KEYCODE_UNKNOWN
            lastMediaTapMs = 0L
            return false
        }
        val device = if (::inputManager.isInitialized) inputManager.getInputDevice(event.deviceId) else null
        if (device != null && device.isVirtual) return false
        val now = SystemClock.uptimeMillis()
        if (code == KeyEvent.KEYCODE_SHIFT_LEFT) shiftUsedWithOtherKey = false
        if (lastMediaTapKey == code && now - lastMediaTapMs in 0..MEDIA_DOUBLE_TAP_MS) {
            pendingBackJob?.cancel()
            pendingBackJob = null
            lastMediaTapKey = KeyEvent.KEYCODE_UNKNOWN
            lastMediaTapMs = 0L
            dispatchMediaKey(mediaKey)
            mediaConsumedKeys.add(code)
            return true
        }
        lastMediaTapKey = code
        lastMediaTapMs = now
        return false
    }

    private fun tryBackspaceAsBack(event: KeyEvent, fgPkg: String?, onHome: Boolean, editable: Boolean): Boolean {
        if (event.keyCode != KeyEvent.KEYCODE_DEL) return false
        if (event.action == KeyEvent.ACTION_UP) {
            return backConsumedKeys.remove(event.keyCode)
        }
        if (event.action == KeyEvent.ACTION_DOWN && event.repeatCount > 0 && event.keyCode in backConsumedKeys) {
            return true
        }
        if (!backspaceAsBack) return false
        if (event.action != KeyEvent.ACTION_DOWN || event.repeatCount != 0) return false
        if (event.isCtrlPressed || event.isAltPressed || event.isMetaPressed || event.isShiftPressed) return false
        if (onHome || editable) return false
        if (fgPkg == null || fgPkg == packageName || fgPkg in backspaceExcluded) return false
        val device = if (::inputManager.isInitialized) inputManager.getInputDevice(event.deviceId) else null
        if (device != null && device.isVirtual) return false
        if (mediaDoubleTap) {
            pendingBackJob?.cancel()
            backConsumedKeys.add(event.keyCode)
            pendingBackJob = scope.launch {
                delay(MEDIA_BACK_DELAY_MS)
                if (lastMediaTapKey == KeyEvent.KEYCODE_DEL) {
                    lastMediaTapKey = KeyEvent.KEYCODE_UNKNOWN
                    lastMediaTapMs = 0L
                }
                performGlobalAction(GLOBAL_ACTION_BACK)
                pendingBackJob = null
            }
            return true
        }
        val performed = performGlobalAction(GLOBAL_ACTION_BACK)
        if (performed) backConsumedKeys.add(event.keyCode)
        return performed
    }

    override fun onKeyEvent(event: KeyEvent): Boolean {
        if (event.deviceId != KeyCharacterMap.VIRTUAL_KEYBOARD) {
            val controller = overlayController
            if (controller != null && (controller.isShowing || event.action == KeyEvent.ACTION_UP)) {
                if (controller.handleKeyEvent(event)) return true
            }
            if (tryQuickSearchHandoff(event)) return true
            if (event.action == KeyEvent.ACTION_UP) {
                val handledMedia = tryMediaDoubleTap(event, editable = false)
                val handledBack = backConsumedKeys.remove(event.keyCode)
                val handledSpeed = speedConsumedKeys.remove(event.keyCode)
                val handledQuick = quickSearchConsumedKeys.remove(event.keyCode)
                if (handledMedia || handledBack || handledSpeed || handledQuick) return true
            } else {
                val locked = isKeyguardLocked()
                val editable = !locked && isEditableInputFocused()
                if (tryMediaDoubleTap(event, editable)) return true
                if (!locked) {
                    val needWindowCheck = homeTypeToSearch || backspaceAsBack || (speedKeysEnabled && speedKeys.isNotEmpty())
                    if (needWindowCheck) {
                        val fgPkg = resolveForegroundAppPackage()
                        val onHome = isOnHomeLauncher(fgPkg)
                        val homeHandlesOwnKeys = fgPkg != null && fgPkg in cachedNativeKeyHomes
                        val onHomeClean = onHome && !editable && !homeHandlesOwnKeys
                        if (tryBackspaceAsBack(event, fgPkg, onHome, editable)) return true
                        if (trySpeedKey(event, onHomeClean)) return true
                        if (tryHomeTypeToSearch(event, onHomeClean)) return true
                    }
                }
            }
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

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        val pkg = event?.packageName?.toString()
        if (!pkg.isNullOrBlank() && pkg != "com.android.systemui" && pkg != packageName) {
            lastAppWindowPackage = pkg
            if (overlayController?.isShowing == true && !isOnHomeLauncher(pkg)) {
                overlayController?.dismiss()
            }
        }
    }

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
        overlayController?.dismiss()
        releaseKeys()
    }

    override fun onDestroy() {
        pendingBackJob?.cancel()
        pendingBackJob = null
        overlayController?.dismiss()
        overlayController = null
        releaseKeys()
        setServiceActive(false)
        scope.cancel()
        super.onDestroy()
    }

    companion object {
        const val INJECT_EVENTS = "android.permission.INJECT_EVENTS"
        const val META_NATIVE_HOME_KEYS = "org.mindone.launcher.NATIVE_HOME_KEYS"
        private const val HOME_CACHE_TTL_MS = 30_000L
        private const val MEDIA_DOUBLE_TAP_MS = 350L
        private const val MEDIA_BACK_DELAY_MS = 260L

        private val _activeFlow = MutableStateFlow(false)
        val activeFlow: StateFlow<Boolean> = _activeFlow.asStateFlow()

        private val _detectedHomeFlow = MutableStateFlow<String?>(null)
        val detectedHomeFlow: StateFlow<String?> = _detectedHomeFlow.asStateFlow()

        @Volatile
        var active = false
            private set

        private fun setServiceActive(value: Boolean) {
            active = value
            _activeFlow.value = value
        }

        fun isNodeEditable(node: AccessibilityNodeInfo): Boolean {
            if (node.isEditable) return true
            if (node.inputType != InputType.TYPE_NULL) return true
            if (node.actionList.any { it.id == AccessibilityNodeInfo.ACTION_SET_TEXT }) return true
            val cls = node.className?.toString().orEmpty()
            return cls.contains("EditText", ignoreCase = true) ||
                cls.contains("AutoComplete", ignoreCase = true) ||
                cls.contains("TextField", ignoreCase = true) ||
                cls.contains("Terminal", ignoreCase = true)
        }
    }
}
