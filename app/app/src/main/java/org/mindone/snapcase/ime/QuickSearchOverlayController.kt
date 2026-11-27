// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import android.accessibilityservice.AccessibilityService
import android.graphics.PixelFormat
import android.util.Log
import android.view.Gravity
import android.view.KeyEvent
import android.view.WindowInsets
import android.view.WindowManager
import androidx.compose.ui.unit.dp
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.ViewCompositionStrategy
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.lifecycle.setViewTreeLifecycleOwner
import androidx.lifecycle.setViewTreeViewModelStoreOwner
import androidx.savedstate.setViewTreeSavedStateRegistryOwner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.mindone.snapcase.data.SettingsRepository
import org.mindone.snapcase.ui.theme.SnapTheme

class QuickSearchOverlayController(
    private val service: AccessibilityService,
    private val scope: CoroutineScope,
) {
    private val windowManager = service.getSystemService(WindowManager::class.java)
    private var overlayView: ComposeView? = null
    private var lifecycleOwner: ImeLifecycleOwner? = null
    private var queryValue by mutableStateOf(TextFieldValue(""))
    private var allApps by mutableStateOf(QuickSearchBridge.cachedApps)
    private var recentPackages by mutableStateOf(QuickSearchBridge.recentPackages())
    private var selectedIndex by mutableIntStateOf(0)
    private val consumedUpKeys = HashSet<Int>()

    val isShowing: Boolean get() = overlayView != null

    fun show(initialChar: Char?): Boolean {
        val text = initialChar?.toString().orEmpty()
        queryValue = TextFieldValue(text, TextRange(text.length))
        selectedIndex = 0
        if (QuickSearchBridge.cachedApps.isNotEmpty()) {
            allApps = QuickSearchBridge.cachedApps
        }
        recentPackages = QuickSearchBridge.recentPackages()
        if (overlayView != null) return true

        val owner = ImeLifecycleOwner().apply {
            onCreate()
            onStart()
        }
        val statusBarPx = runCatching {
            windowManager.currentWindowMetrics.windowInsets
                .getInsetsIgnoringVisibility(WindowInsets.Type.statusBars()).top
        }.getOrDefault(0)
        val extraTopDp = (statusBarPx / service.resources.displayMetrics.density).dp

        val view = ComposeView(service).apply {
            setViewTreeLifecycleOwner(owner)
            setViewTreeViewModelStoreOwner(owner)
            setViewTreeSavedStateRegistryOwner(owner)
            setViewCompositionStrategy(ViewCompositionStrategy.DisposeOnViewTreeLifecycleDestroyed)
            setContent {
                SnapTheme {
                    QuickSearchCard(
                        query = queryValue,
                        allApps = allApps,
                        recentPackages = recentPackages,
                        selectedIndex = selectedIndex,
                        readOnlyDisplay = true,
                        extraTopPadding = extraTopDp,
                        onAppsLoaded = { loaded ->
                            allApps = loaded
                            QuickSearchBridge.cachedApps = loaded
                        },
                        onQueryChange = { next ->
                            queryValue = next
                            selectedIndex = 0
                        },
                        onLaunch = { app -> launchApp(app) },
                        onLaunchSelected = { launchAt(selectedIndex) },
                        onDismiss = { dismiss() },
                        loadIcon = { pkg -> QuickSearchActivity.loadAppIcon(service, pkg) },
                    )
                }
            }
        }

        val params = WindowManager.LayoutParams(
            WindowManager.LayoutParams.MATCH_PARENT,
            WindowManager.LayoutParams.MATCH_PARENT,
            WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.START
        }

        return try {
            windowManager.addView(view, params)
            overlayView = view
            lifecycleOwner = owner
            QuickSearchBridge.setSessionActive(true)
            true
        } catch (error: Exception) {
            Log.w(TAG, "Overlay attach failed", error)
            owner.onDestroy()
            false
        }
    }

    fun dismiss() {
        val view = overlayView
        overlayView = null
        val owner = lifecycleOwner
        lifecycleOwner = null
        QuickSearchBridge.setSessionActive(false)
        if (view != null) {
            runCatching { windowManager.removeViewImmediate(view) }
        }
        owner?.onDestroy()
    }

    fun handleKeyEvent(event: KeyEvent): Boolean {
        if (event.action == KeyEvent.ACTION_UP) {
            if (consumedUpKeys.remove(event.keyCode)) return true
            return isShowing
        }
        if (!isShowing) return false
        if (event.action != KeyEvent.ACTION_DOWN) return true
        val code = event.keyCode
        consumedUpKeys.add(code)
        when (code) {
            KeyEvent.KEYCODE_ESCAPE, KeyEvent.KEYCODE_BACK -> {
                if (event.repeatCount == 0) dismiss()
                return true
            }
            KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> {
                if (event.repeatCount == 0) launchAt(selectedIndex)
                return true
            }
            KeyEvent.KEYCODE_DPAD_DOWN -> {
                moveSelection(1)
                return true
            }
            KeyEvent.KEYCODE_DPAD_UP -> {
                moveSelection(-1)
                return true
            }
            KeyEvent.KEYCODE_TAB -> {
                moveSelection(if (event.isShiftPressed) -1 else 1)
                return true
            }
            KeyEvent.KEYCODE_DEL -> {
                if (queryValue.text.isEmpty()) {
                    if (event.repeatCount == 0) dismiss()
                } else {
                    val trimmed = queryValue.text.dropLast(1)
                    queryValue = TextFieldValue(trimmed, TextRange(trimmed.length))
                    selectedIndex = 0
                }
                return true
            }
            KeyEvent.KEYCODE_SPACE -> {
                if (queryValue.text.isNotEmpty() && !queryValue.text.endsWith(' ')) {
                    val next = queryValue.text + " "
                    queryValue = TextFieldValue(next, TextRange(next.length))
                    selectedIndex = 0
                }
                return true
            }
        }
        if ((event.isAltPressed || event.isCtrlPressed) && !event.isMetaPressed && event.repeatCount == 0) {
            val quickIdx = when (code) {
                KeyEvent.KEYCODE_1 -> 0
                KeyEvent.KEYCODE_2 -> 1
                KeyEvent.KEYCODE_3 -> 2
                KeyEvent.KEYCODE_4 -> 3
                KeyEvent.KEYCODE_5 -> 4
                else -> -1
            }
            if (quickIdx >= 0) {
                launchAt(quickIdx)
                return true
            }
        }
        if (event.isCtrlPressed || event.isAltPressed || event.isMetaPressed) return true
        val ch = QuickLauncher.extractSearchChar(code, event.unicodeChar, event.isShiftPressed)
        if (ch != null) {
            val next = queryValue.text + ch
            queryValue = TextFieldValue(next, TextRange(next.length))
            selectedIndex = 0
        }
        return true
    }

    private fun currentMatches(): List<LaunchableApp> =
        QuickLauncher.searchApps(allApps, queryValue.text, recentPackages)

    private fun moveSelection(delta: Int) {
        val matches = currentMatches()
        if (matches.isEmpty()) return
        selectedIndex = (selectedIndex + delta).coerceIn(0, matches.lastIndex)
    }

    private fun launchAt(index: Int): Boolean {
        val app = currentMatches().getOrNull(index) ?: return false
        return launchApp(app)
    }

    private fun launchApp(app: LaunchableApp): Boolean {
        val ok = QuickLauncher.launch(service, app)
        if (ok) {
            QuickSearchBridge.recordLaunch(app.packageName)
            scope.launch(Dispatchers.IO) {
                runCatching { SettingsRepository.from(service.applicationContext).recordQuickSearchRecent(app.packageName) }
            }
            dismiss()
        }
        return ok
    }

    companion object {
        private const val TAG = "QuickSearchOverlay"
    }
}
