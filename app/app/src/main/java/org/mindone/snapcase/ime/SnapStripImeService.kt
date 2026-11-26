// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import android.content.res.Configuration
import android.graphics.Color
import android.inputmethodservice.InputMethodService
import android.os.PowerManager
import android.os.VibrationAttributes
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.text.InputType
import android.view.Gravity
import android.view.KeyEvent
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethod
import android.view.inputmethod.InputMethodManager
import android.view.inputmethod.InputMethodSubtype
import android.widget.FrameLayout
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.ViewCompositionStrategy
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.setViewTreeLifecycleOwner
import androidx.lifecycle.setViewTreeViewModelStoreOwner
import androidx.savedstate.setViewTreeSavedStateRegistryOwner
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import org.mindone.snapcase.data.AppSettings
import org.mindone.snapcase.data.SettingsRepository
import org.mindone.snapcase.ime.dict.DictionaryPackManager
import org.mindone.snapcase.ime.dict.PersonalDictionary
import org.mindone.snapcase.ime.dict.SuggestionEngine
import org.mindone.snapcase.ime.ui.SnapStripRoot
import org.mindone.snapcase.kumi.KumiLiveSnapshot
import org.mindone.snapcase.kumi.SnapLiveMirror
import org.mindone.snapcase.system.DeviceState
import org.mindone.snapcase.ui.theme.SnapTheme

class SnapStripImeService : InputMethodService() {
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val lifecycleOwner = ImeLifecycleOwner()

    private lateinit var dictManager: DictionaryPackManager
    private lateinit var personalDict: PersonalDictionary
    private lateinit var emojiCatalog: EmojiCatalog
    private lateinit var clipboardStore: ClipboardStore
    private lateinit var processor: ImeKeyProcessor
    private var detachClipboard: (() -> Unit)? = null

    private val uiState = MutableStateFlow(ImeUiState())
    private val showQuickExtras = MutableStateFlow(true)
    @Volatile private var stripEnabled = true
    @Volatile private var alwaysShowStrip = true
    @Volatile private var showOnTypingOnly = true
    @Volatile private var shiftSpaceSwitch = true
    @Volatile private var hapticMode = HapticMode.EVENTS
    @Volatile private var hapticDurationMs = 5
    private var editableInput = false
    private var typingActive = false
    private var userDismissed = false
    private var hidingSelf = false
    private var physicalShiftHeld = false
    private var physicalCapsLock = false
    private var actualPasswordField = false
    private var numericField = false
    private var literalPunctuationField = false
    private var shiftUsedInChord = false
    private var shiftPressedWithAlt = false
    private var longPressConsumedKeyCode = KeyEvent.KEYCODE_UNKNOWN
    private val consumedDownKeys = HashSet<Int>()
    private var kumiSnapshot = KumiLiveSnapshot()
    private var inputRoot: FrameLayout? = null
    private var lastBarHeight = -1
    private var vibrator: Vibrator? = null
    private var powerManager: PowerManager? = null

    private val inputTarget = object : InputTarget {
        override fun setComposing(text: String) {
            currentInputConnection?.setComposingText(text, 1)
        }

        override fun commit(text: String) {
            currentInputConnection?.commitText(text, 1)
        }

        override fun finishComposing() {
            currentInputConnection?.finishComposingText()
        }

        override fun deleteSurrounding(before: Int, after: Int) {
            currentInputConnection?.deleteSurroundingText(before, after)
        }

        override fun textBeforeCursor(length: Int): String =
            currentInputConnection?.getTextBeforeCursor(length, 0)?.toString().orEmpty()
    }

    override fun onCreate() {
        super.onCreate()
        lifecycleOwner.onCreate()
        powerManager = getSystemService(PowerManager::class.java)
        vibrator = getSystemService(VibratorManager::class.java)?.defaultVibrator
        dictManager = DictionaryPackManager.shared(applicationContext)
        personalDict = PersonalDictionary(File(applicationContext.filesDir, "ime"))
        emojiCatalog = EmojiCatalog(applicationContext)
        clipboardStore = ClipboardStore(ClipboardStore.defaultFile(applicationContext))
        detachClipboard = clipboardStore.attachSystemClipboard(applicationContext)
        processor = ImeKeyProcessor(
            engine = SuggestionEngine(personalDict),
            dictProvider = { lang -> dictManager.get(lang.code) },
            personal = personalDict,
            jaDictProvider = { dictManager.getJapanese() },
        )

        serviceScope.launch(Dispatchers.IO) {
            dictManager.get("en")
            dictManager.get("ru")
            dictManager.getJapanese()
        }

        serviceScope.launch {
            SettingsRepository.from(applicationContext).settings.collectLatest { applySettings(it) }
        }
        serviceScope.launch {
            SnapLiveMirror.snapshot.collectLatest { snap ->
                kumiSnapshot = snap
                publishState()
            }
        }
        serviceScope.launch {
            clipboardStore.items.collectLatest { publishState() }
        }
    }

    override fun onEvaluateFullscreenMode(): Boolean = false

    override fun onEvaluateInputViewShown(): Boolean {
        super.onEvaluateInputViewShown()
        if (!stripEnabled || !editableInput || actualPasswordField) return false
        if (!hasHardwareKeyboard()) return alwaysShowStrip
        if (userDismissed) return false
        return if (showOnTypingOnly) typingActive else true
    }

    override fun onShowInputRequested(flags: Int, configChange: Boolean): Boolean {
        if (!stripEnabled || !editableInput || actualPasswordField) return false
        @Suppress("DEPRECATION")
        if ((flags and InputMethod.SHOW_EXPLICIT) != 0 && (!showOnTypingOnly || !hasHardwareKeyboard() || userDismissed)) {
            userDismissed = false
            typingActive = true
            return true
        }
        return onEvaluateInputViewShown()
    }

    private fun styleImeWindow() {
        val w = window?.window ?: return
        WindowCompat.setDecorFitsSystemWindows(w, false)
        w.setBackgroundDrawableResource(android.R.color.transparent)
        w.decorView.setBackgroundColor(Color.TRANSPARENT)
        w.findViewById<View>(android.R.id.inputArea)?.setBackgroundColor(Color.TRANSPARENT)
        @Suppress("DEPRECATION")
        w.navigationBarColor = Color.TRANSPARENT
        w.isNavigationBarContrastEnforced = false
        val isDark = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
        WindowInsetsControllerCompat(w, w.decorView).isAppearanceLightNavigationBars = !isDark
    }

    private fun triggerHaptic(isKeyPress: Boolean) {
        val mode = hapticMode
        if (mode == HapticMode.OFF) return
        if (isKeyPress && mode != HapticMode.ALL_KEYS) return
        if (powerManager?.isPowerSaveMode == true) return
        val vib = vibrator ?: return
        if (!vib.hasVibrator()) return
        val ms = hapticDurationMs.coerceIn(4, 15).toLong()
        val effect = VibrationEffect.createOneShot(ms, VibrationEffect.DEFAULT_AMPLITUDE)
        val attrs = VibrationAttributes.Builder()
            .setUsage(VibrationAttributes.USAGE_TOUCH)
            .build()
        vib.vibrate(effect, attrs)
    }

    private fun revealStripForTyping(force: Boolean = false) {
        if (!stripEnabled || !editableInput || (!force && actualPasswordField)) return
        if (force) userDismissed = false
        if (userDismissed) return
        hidingSelf = false
        val needsShow = !typingActive || !isInputViewShown || window?.isShowing != true
        typingActive = true
        if (needsShow) {
            updateInputViewShown()
            requestShowSelf(0)
        }
    }

    override fun onComputeInsets(outInsets: Insets) {
        super.onComputeInsets(outInsets)
        val root = inputRoot ?: return
        if (root.visibility == View.VISIBLE && root.height > 0) {
            val loc = IntArray(2)
            root.getLocationInWindow(loc)
            val top = loc[1].coerceAtLeast(0)
            outInsets.contentTopInsets = top
            outInsets.visibleTopInsets = top
            outInsets.touchableInsets = Insets.TOUCHABLE_INSETS_VISIBLE
        }
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        styleImeWindow()
        updateInputViewShown()
        publishState()
    }

    override fun onCreateInputView(): View {
        lifecycleOwner.onStart()
        styleImeWindow()
        val decor = window?.window?.decorView
        decor?.setViewTreeLifecycleOwner(lifecycleOwner)
        decor?.setViewTreeViewModelStoreOwner(lifecycleOwner)
        decor?.setViewTreeSavedStateRegistryOwner(lifecycleOwner)

        val compose = ComposeView(this).apply {
            layoutParams = FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
                Gravity.BOTTOM,
            )
            setViewTreeLifecycleOwner(lifecycleOwner)
            setViewTreeViewModelStoreOwner(lifecycleOwner)
            setViewTreeSavedStateRegistryOwner(lifecycleOwner)
            setViewCompositionStrategy(ViewCompositionStrategy.DisposeOnViewTreeLifecycleDestroyed)
            setContent {
                val state by uiState.collectAsState()
                val extras by showQuickExtras.collectAsState()
                SnapTheme {
                    SnapStripRoot(
                        state = state,
                        emojiCatalog = emojiCatalog,
                        showQuickExtras = extras,
                        onPickSuggestion = { word ->
                            triggerHaptic(isKeyPress = false)
                            processor.pickSuggestion(word, inputTarget)
                            publishState()
                        },
                        onForgetSuggestion = { word ->
                            processor.forgetSuggestion(word)
                            publishState()
                        },
                        onPickVariation = { variant ->
                            triggerHaptic(isKeyPress = false)
                            processor.pickVariation(variant, inputTarget)
                            publishState()
                        },
                        onInsertDirect = { text ->
                            triggerHaptic(isKeyPress = false)
                            processor.insertDirectText(text, inputTarget)
                            publishState()
                        },
                        onPickEmoji = { glyph ->
                            triggerHaptic(isKeyPress = false)
                            emojiCatalog.recordRecent(glyph)
                            processor.commitEmoji(glyph, inputTarget)
                            publishState()
                        },
                        onToggleDrawer = { drawer ->
                            triggerHaptic(isKeyPress = false)
                            processor.setDrawer(drawer)
                            publishState()
                        },
                        onCycleLang = {
                            triggerHaptic(isKeyPress = false)
                            processor.cycleLanguage(inputTarget)
                            publishState()
                        },
                        onLongPressLang = {
                            getSystemService(InputMethodManager::class.java)?.showInputMethodPicker()
                        },
                        onSwitchVirtualKeyboard = {
                            if (!switchToNextInputMethod(false)) {
                                getSystemService(InputMethodManager::class.java)?.showInputMethodPicker()
                            }
                        },
                        onPinClip = { clipboardStore.togglePin(it) },
                        onDeleteClip = { clipboardStore.remove(it) },
                        onClearClips = { clipboardStore.clearUnpinned() },
                    )
                }
            }
        }

        val container = FrameLayout(this).apply {
            layoutParams = ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
            )
            addView(compose)
            addOnLayoutChangeListener { view, _, top, _, bottom, _, _, _, _ ->
                val h = bottom - top
                if (h != lastBarHeight && h > 0) {
                    lastBarHeight = h
                    view.post { window?.window?.decorView?.requestLayout() }
                }
            }
        }
        inputRoot = container
        lastBarHeight = -1
        return container
    }

    override fun onWindowShown() {
        super.onWindowShown()
        styleImeWindow()
        lifecycleOwner.onStart()
    }

    override fun onWindowHidden() {
        lifecycleOwner.onStop()
        if (!hidingSelf && editableInput && (typingActive || !showOnTypingOnly)) {
            userDismissed = true
        }
        hidingSelf = false
        typingActive = false
        physicalShiftHeld = false
        processor.closeDrawer()
        publishState()
        super.onWindowHidden()
    }

    override fun onStartInput(attribute: EditorInfo?, restarting: Boolean) {
        super.onStartInput(attribute, restarting)
        editableInput = isEditableInput(attribute)
        actualPasswordField = isActualPassword(attribute)
        numericField = isNumericField(attribute)
        literalPunctuationField = isLiteralPunctuationField(attribute)
        val isPassword = isSensitiveField(attribute)
        physicalShiftHeld = false
        shiftUsedInChord = false
        shiftPressedWithAlt = false
        longPressConsumedKeyCode = KeyEvent.KEYCODE_UNKNOWN
        consumedDownKeys.clear()
        if (!editableInput) {
            userDismissed = false
            typingActive = false
            processor.finishInput()
            updateInputViewShown()
            if (isInputViewShown || window?.isShowing == true) {
                hidingSelf = true
                requestHideSelf(0)
            }
            publishState()
            return
        }
        processor.startInput(isPassword, if (actualPasswordField) "" else inputTarget.textBeforeCursor(32))
        if (!restarting) {
            userDismissed = false
            typingActive = false
            updateInputViewShown()
            if (showOnTypingOnly && hasHardwareKeyboard() && (isInputViewShown || window?.isShowing == true)) {
                hidingSelf = true
                requestHideSelf(0)
            }
        } else if (!isInputViewShown) {
            userDismissed = false
        }
        publishState()
    }

    override fun onStartInputView(info: EditorInfo?, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        styleImeWindow()
        lifecycleOwner.onStart()
        publishState()
    }

    override fun onFinishInput() {
        editableInput = false
        hidingSelf = false
        userDismissed = false
        typingActive = false
        physicalShiftHeld = false
        shiftUsedInChord = false
        shiftPressedWithAlt = false
        longPressConsumedKeyCode = KeyEvent.KEYCODE_UNKNOWN
        consumedDownKeys.clear()
        processor.finishInput()
        publishState()
        super.onFinishInput()
    }

    override fun onUpdateSelection(
        oldSelStart: Int,
        oldSelEnd: Int,
        newSelStart: Int,
        newSelEnd: Int,
        candidatesStart: Int,
        candidatesEnd: Int,
    ) {
        super.onUpdateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd, candidatesStart, candidatesEnd)
        if (processor.hasComposing()) {
            if (candidatesStart < 0 || candidatesEnd < 0 || newSelStart != candidatesEnd || newSelEnd != candidatesEnd) {
                processor.onCursorMovedExternally(inputTarget)
                publishState()
            }
        } else if (newSelStart != oldSelStart || newSelEnd != oldSelEnd) {
            processor.onSelectionChanged(if (actualPasswordField) "" else inputTarget.textBeforeCursor(32))
            publishState()
        }
    }

    override fun onCurrentInputMethodSubtypeChanged(newSubtype: InputMethodSubtype?) {
        super.onCurrentInputMethodSubtypeChanged(newSubtype)
        val locale = newSubtype?.locale.orEmpty()
        StripLang.entries.firstOrNull { locale.startsWith(it.code, ignoreCase = true) }
            ?.let { processor.setLanguage(it, inputTarget) }
        publishState()
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        if (!stripEnabled || currentInputConnection == null || !isEditableInput(currentInputEditorInfo)) {
            return super.onKeyDown(keyCode, event)
        }
        if (handleKeyDown(keyCode, event)) {
            consumedDownKeys.add(keyCode)
            return true
        }
        return super.onKeyDown(keyCode, event)
    }

    private fun handleKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        if (event.repeatCount == 0) {
            longPressConsumedKeyCode = KeyEvent.KEYCODE_UNKNOWN
        }
        if (keyCode != KeyEvent.KEYCODE_SHIFT_LEFT && keyCode != KeyEvent.KEYCODE_SHIFT_RIGHT && (physicalShiftHeld || event.isShiftPressed)) {
            shiftUsedInChord = true
        }
        if (physicalCapsLock != event.isCapsLockOn) {
            physicalCapsLock = event.isCapsLockOn
        }

        val isLangSwitchChord = keyCode == KeyEvent.KEYCODE_LANGUAGE_SWITCH ||
            (keyCode == KeyEvent.KEYCODE_SPACE && (event.isCtrlPressed || event.isMetaPressed || (shiftSpaceSwitch && event.isShiftPressed)))
        if (isLangSwitchChord) {
            if (event.repeatCount == 0) {
                triggerHaptic(isKeyPress = false)
                processor.cycleLanguage(inputTarget)
                publishState()
                revealStripForTyping(force = true)
            }
            return true
        }

        if ((keyCode == KeyEvent.KEYCODE_ALT_LEFT || keyCode == KeyEvent.KEYCODE_ALT_RIGHT) && event.isShiftPressed && event.repeatCount == 0) {
            shiftUsedInChord = true
            triggerHaptic(isKeyPress = false)
            processor.cycleLanguage(inputTarget)
            publishState()
            revealStripForTyping(force = true)
            return true
        }
        if ((keyCode == KeyEvent.KEYCODE_SHIFT_LEFT || keyCode == KeyEvent.KEYCODE_SHIFT_RIGHT) && event.isAltPressed && event.repeatCount == 0) {
            shiftPressedWithAlt = true
            shiftUsedInChord = true
            triggerHaptic(isKeyPress = false)
            processor.cycleLanguage(inputTarget)
            publishState()
            revealStripForTyping(force = true)
            return true
        }

        if (keyCode == KeyEvent.KEYCODE_SPACE && event.isAltPressed && !event.isShiftPressed && !event.isCtrlPressed && !event.isMetaPressed) {
            if (event.repeatCount == 0) {
                if (!isInputViewShown || window?.isShowing != true) {
                    revealStripForTyping(force = true)
                } else if (processor.acceptSuggestionAt(0, inputTarget)) {
                    publishState()
                }
            }
            return true
        }

        if ((event.isCtrlPressed || event.isAltPressed) && !event.isMetaPressed && event.repeatCount == 0) {
            val pickIdx = when (keyCode) {
                KeyEvent.KEYCODE_1 -> 0
                KeyEvent.KEYCODE_2 -> 1
                KeyEvent.KEYCODE_3 -> 2
                else -> -1
            }
            if (pickIdx >= 0 && processor.acceptSuggestionAt(pickIdx, inputTarget)) {
                publishState()
                revealStripForTyping()
                return true
            }
        }

        if (keyCode == KeyEvent.KEYCODE_SHIFT_LEFT || keyCode == KeyEvent.KEYCODE_SHIFT_RIGHT) {
            if (event.repeatCount == 0 && !shiftPressedWithAlt) {
                physicalShiftHeld = true
                shiftUsedInChord = false
                publishState()
            }
            return false
        }

        if (KeyEvent.isModifierKey(keyCode)) {
            return false
        }

        if (event.isCtrlPressed || event.isMetaPressed || event.isAltPressed) {
            processor.onCursorMovedExternally(inputTarget)
            publishState()
            return false
        }

        if (keyCode == KeyEvent.KEYCODE_TAB && uiState.value.activeDrawer == StripDrawer.NONE &&
            (processor.hasComposing() || processor.hasSelectedSuggestion()) && processor.hasSuggestions()
        ) {
            if (processor.moveSuggestionSelection(if (event.isShiftPressed) -1 else 1)) {
                publishState()
                revealStripForTyping()
                return true
            }
        }

        if (!event.isShiftPressed && keyCode == KeyEvent.KEYCODE_DPAD_RIGHT && uiState.value.activeDrawer == StripDrawer.NONE &&
            (processor.hasComposing() || processor.hasSelectedSuggestion()) && processor.hasSuggestions()
        ) {
            if (processor.moveSuggestionSelection(1)) {
                publishState()
                return true
            }
        }

        if (!event.isShiftPressed && keyCode == KeyEvent.KEYCODE_DPAD_LEFT && uiState.value.activeDrawer == StripDrawer.NONE &&
            processor.hasSelectedSuggestion()
        ) {
            if (processor.moveSuggestionSelection(-1)) {
                publishState()
                return true
            }
        }

        if (!event.isShiftPressed && keyCode == KeyEvent.KEYCODE_DPAD_UP && uiState.value.activeDrawer == StripDrawer.NONE &&
            processor.hasComposing() && !processor.hasSelectedSuggestion() && processor.hasSuggestions()
        ) {
            if (processor.selectFirstSuggestion()) {
                publishState()
                return true
            }
        }

        if (!event.isShiftPressed && (keyCode == KeyEvent.KEYCODE_DPAD_DOWN || keyCode == KeyEvent.KEYCODE_ESCAPE) &&
            uiState.value.activeDrawer == StripDrawer.NONE && processor.hasSelectedSuggestion()
        ) {
            if (processor.clearSuggestionSelection()) {
                publishState()
                return true
            }
        }

        if (physicalShiftHeld != event.isShiftPressed) {
            physicalShiftHeld = event.isShiftPressed
        }

        val activeLang = processor.currentLanguage()
        val isCaseKeyboard = DeviceState.isCaseKeyboard(event.device)
        val translated = PhysicalLayoutTranslator.translate(
            keyCode = keyCode,
            shifted = event.isShiftPressed,
            rawUnicode = event.unicodeChar,
            lang = activeLang,
            compactCaseKeyboard = isCaseKeyboard,
            capsLock = event.isCapsLockOn,
            literalPunctuation = literalPunctuationField,
        )

        if (uiState.value.activeDrawer == StripDrawer.EMOJI) {
            val firstEmoji = emojiCatalog.search(uiState.value.emojiQuery).firstOrNull()?.glyph
            val consumed = processor.handleEmojiKey(
                ch = translated,
                isBackspace = keyCode == KeyEvent.KEYCODE_DEL,
                isEnter = keyCode == KeyEvent.KEYCODE_ENTER,
                isEscape = keyCode == KeyEvent.KEYCODE_ESCAPE || keyCode == KeyEvent.KEYCODE_BACK,
                firstEmoji = firstEmoji,
                target = inputTarget,
            )
            if (consumed) {
                if (keyCode == KeyEvent.KEYCODE_ENTER && !firstEmoji.isNullOrEmpty()) {
                    emojiCatalog.recordRecent(firstEmoji)
                }
                publishState()
                return true
            }
        }

        if ((keyCode == KeyEvent.KEYCODE_ESCAPE || keyCode == KeyEvent.KEYCODE_BACK) && uiState.value.activeDrawer != StripDrawer.NONE) {
            processor.closeDrawer()
            publishState()
            return true
        }

        if (keyCode == KeyEvent.KEYCODE_DEL) {
            if (event.repeatCount == 0) triggerHaptic(isKeyPress = true)
            val handled = processor.onBackspace(inputTarget)
            publishState()
            return handled
        }

        if (keyCode == KeyEvent.KEYCODE_ENTER) {
            if (event.repeatCount == 0) triggerHaptic(isKeyPress = true)
            val handled = processor.onEnter(inputTarget)
            publishState()
            return handled
        }

        if (translated != null && !translated.isISOControl()) {
            if (event.repeatCount == 1) {
                if (processor.onLongPressRepeat(
                        translated,
                        inputTarget,
                        keyCode = keyCode,
                        compactCaseKeyboard = isCaseKeyboard,
                        numericField = numericField,
                    )
                ) {
                    longPressConsumedKeyCode = keyCode
                    triggerHaptic(isKeyPress = false)
                    publishState()
                    if (uiState.value.variations.isNotEmpty()) {
                        revealStripForTyping(force = true)
                    }
                    return true
                }
            }
            if (event.repeatCount > 1 && longPressConsumedKeyCode == keyCode) {
                return true
            }
            if (event.repeatCount == 0) triggerHaptic(isKeyPress = true)
            val handled = processor.onCharTyped(
                translated,
                event.isShiftPressed || event.isCapsLockOn,
                inputTarget,
                autoDetectLang = false,
                isRepeat = event.repeatCount > 0,
            )
            publishState()
            if (handled) {
                revealStripForTyping()
                return true
            }
            if (uiState.value.isPasswordField || (event.unicodeChar != 0 && event.unicodeChar.toChar() != translated)) {
                val out = if (uiState.value.isPasswordField) {
                    processor.consumeEffectiveUpper(translated, event.isShiftPressed || event.isCapsLockOn)
                } else {
                    translated
                }
                inputTarget.commit(out.toString())
                publishState()
                revealStripForTyping()
                return true
            }
            revealStripForTyping()
        }

        return false
    }

    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean {
        if (keyCode == longPressConsumedKeyCode) {
            longPressConsumedKeyCode = KeyEvent.KEYCODE_UNKNOWN
        }
        if (keyCode == KeyEvent.KEYCODE_SHIFT_LEFT || keyCode == KeyEvent.KEYCODE_SHIFT_RIGHT) {
            val wasCleanTap = !shiftUsedInChord && !shiftPressedWithAlt
            shiftPressedWithAlt = false
            shiftUsedInChord = false
            physicalShiftHeld = event.isShiftPressed
            if (wasCleanTap && !uiState.value.isPasswordField && processor.currentLanguage() != StripLang.JA) {
                processor.onShiftTap()
            }
            publishState()
        } else if (keyCode == KeyEvent.KEYCODE_ALT_LEFT || keyCode == KeyEvent.KEYCODE_ALT_RIGHT) {
            shiftPressedWithAlt = false
        }
        if (consumedDownKeys.remove(keyCode)) {
            return true
        }
        return super.onKeyUp(keyCode, event)
    }

    override fun onDestroy() {
        detachClipboard?.invoke()
        detachClipboard = null
        lifecycleOwner.onDestroy()
        serviceScope.cancel()
        super.onDestroy()
    }

    private fun applySettings(s: AppSettings) {
        stripEnabled = s.imeStripEnabled
        alwaysShowStrip = s.imeAlwaysShowStrip
        showOnTypingOnly = s.imeShowOnTypingOnly
        shiftSpaceSwitch = s.imeShiftSpaceSwitch
        hapticMode = s.imeHapticMode
        hapticDurationMs = s.imeHapticDurationMs
        showQuickExtras.value = s.imeShowQuickExtras
        val enabledLangs = s.imeEnabledLangs.map(StripLang::fromCode).distinct().ifEmpty { listOf(StripLang.EN, StripLang.RU) }
        serviceScope.launch(Dispatchers.IO) {
            enabledLangs.forEach { lang ->
                if (lang == StripLang.JA) dictManager.getJapanese() else dictManager.get(lang.code)
            }
        }
        processor.updateConfig(
            ImeConfig(
                stripEnabled = s.imeStripEnabled,
                autoCorrect = s.imeAutoCorrect,
                autoCapitalize = s.imeAutoCapitalize,
                doubleSpacePeriod = s.imeDoubleSpacePeriod,
                stickyShift = s.imeStickyShift,
                longPressBehavior = s.imeLongPressBehavior,
                showQuickExtras = s.imeShowQuickExtras,
                defaultLang = StripLang.fromCode(s.imeDefaultLang),
                enabledLangs = enabledLangs,
                shiftSpaceSwitch = s.imeShiftSpaceSwitch,
                snippets = s.imeSnippets,
            ),
        )
        updateInputViewShown()
        publishState()
    }

    private fun isEditableInput(info: EditorInfo?): Boolean =
        info != null && info.inputType != InputType.TYPE_NULL

    private fun hasHardwareKeyboard(): Boolean =
        resources.configuration.keyboard != Configuration.KEYBOARD_NOKEYS

    private fun publishState() {
        uiState.value = processor.snapshot(
            kumiConnected = kumiSnapshot.connected,
            kumiLayer = kumiSnapshot.activeLayer,
            kumiCapsWord = kumiSnapshot.capsWord,
            hasHardwareKeyboard = hasHardwareKeyboard(),
            clipboard = clipboardStore.items.value,
            physicalShiftHeld = physicalShiftHeld,
            physicalCapsLock = physicalCapsLock,
        )
    }

    private fun isNumericField(info: EditorInfo?): Boolean {
        if (info == null) return false
        val cls = info.inputType and InputType.TYPE_MASK_CLASS
        return cls == InputType.TYPE_CLASS_NUMBER || cls == InputType.TYPE_CLASS_PHONE
    }

    private fun isLiteralPunctuationField(info: EditorInfo?): Boolean {
        if (info == null) return false
        val type = info.inputType
        val cls = type and InputType.TYPE_MASK_CLASS
        val variation = type and InputType.TYPE_MASK_VARIATION
        if (cls == InputType.TYPE_CLASS_NUMBER || cls == InputType.TYPE_CLASS_PHONE) return true
        return cls == InputType.TYPE_CLASS_TEXT && (
            variation == InputType.TYPE_TEXT_VARIATION_PASSWORD ||
                variation == InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD ||
                variation == InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD ||
                variation == InputType.TYPE_TEXT_VARIATION_URI ||
                variation == InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS ||
                variation == InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS
            )
    }

    private fun isActualPassword(info: EditorInfo?): Boolean {
        if (info == null) return false
        val type = info.inputType
        val cls = type and InputType.TYPE_MASK_CLASS
        val variation = type and InputType.TYPE_MASK_VARIATION
        if (cls == InputType.TYPE_CLASS_NUMBER && variation == InputType.TYPE_NUMBER_VARIATION_PASSWORD) return true
        return cls == InputType.TYPE_CLASS_TEXT && (
            variation == InputType.TYPE_TEXT_VARIATION_PASSWORD ||
                variation == InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD ||
                variation == InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD
            )
    }

    private fun isSensitiveField(info: EditorInfo?): Boolean {
        if (info == null) return false
        val type = info.inputType
        val cls = type and InputType.TYPE_MASK_CLASS
        val variation = type and InputType.TYPE_MASK_VARIATION
        if (cls == InputType.TYPE_CLASS_NUMBER) return true
        if (cls == InputType.TYPE_CLASS_TEXT) {
            if (variation == InputType.TYPE_TEXT_VARIATION_PASSWORD ||
                variation == InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD ||
                variation == InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD ||
                variation == InputType.TYPE_TEXT_VARIATION_URI ||
                variation == InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS ||
                variation == InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS
            ) return true
            if (type and InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS != 0) return true
        }
        return false
    }
}
