// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.annotation.SuppressLint
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbManager
import android.os.Bundle
import android.os.SystemClock
import android.view.KeyEvent
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import androidx.core.content.ContextCompat
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.mindone.snapcase.BuildConfig
import org.mindone.snapcase.R
import org.mindone.snapcase.bridge.KumiBridgeProvider
import org.mindone.snapcase.ime.ui.ImeSettingsScreen
import org.mindone.snapcase.kumi.HomeKind
import org.mindone.snapcase.stock.UserMessage
import org.mindone.snapcase.transport.CaseKind
import org.mindone.snapcase.ui.theme.SnapTheme

data class DeepLinkRoute(val screen: String, val eqSlot: Int? = null, val nonce: Long = System.nanoTime())

class MainActivity : ComponentActivity() {
    private val appViewModel: AppViewModel by viewModels()
    private val keyBus = ScreenKeyBus()
    private val _deepLink = MutableStateFlow<DeepLinkRoute?>(null)
    private val deepLink: StateFlow<DeepLinkRoute?> = _deepLink.asStateFlow()
    private val usbReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            appViewModel.refresh()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        consumeRouteIntent(intent)
        setContent {
            SnapTheme {
                SnapApp(
                    appViewModel,
                    keyBus,
                    deepLink,
                    onDeepLinkConsumed = { _deepLink.value = null },
                    onExit = { finish() },
                )
            }
        }
    }

    override fun onStart() {
        super.onStart()
        val filter = IntentFilter().apply {
            addAction(UsbManager.ACTION_USB_DEVICE_ATTACHED)
            addAction(UsbManager.ACTION_USB_DEVICE_DETACHED)
        }
        ContextCompat.registerReceiver(this, usbReceiver, filter, ContextCompat.RECEIVER_NOT_EXPORTED)
    }

    override fun onStop() {
        runCatching { unregisterReceiver(usbReceiver) }
        super.onStop()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        consumeRouteIntent(intent)
        appViewModel.refresh()
    }

    private fun consumeRouteIntent(intent: Intent?) {
        if (intent?.action != KumiBridgeProvider.ACTION_OPEN_SCREEN) return
        val target = intent.getStringExtra(KumiBridgeProvider.EXTRA_SCREEN)?.trim()?.lowercase().orEmpty()
        if (target !in ALLOWED_DEEP_LINKS) return
        val slot = intent.getIntExtra(KumiBridgeProvider.EXTRA_EQ_SLOT, -1).takeIf { it in 0..3 }
        _deepLink.value = DeepLinkRoute(target, slot)
    }

    @SuppressLint("RestrictedApi")
    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        val imm = getSystemService(InputMethodManager::class.java)
        if (keyBus.dispatch(event, currentFocus is EditText || imm?.isAcceptingText == true)) return true
        return super.dispatchKeyEvent(event)
    }
}

enum class Screen(@StringRes val title: Int) {
    HOME(R.string.app_title),
    KEYS(R.string.keys_title),
    TESTER(R.string.kb_tester_title),
    TECH(R.string.tech_title),
    LEGACY(R.string.legacy_title),
    KUMI_AUDIO(R.string.kumi_audio_title),
    KUMI_EQ(R.string.kumi_eq_title),
    KUMI_KEYS(R.string.kumi_keys_title),
    KUMI_FIRMWARE(R.string.kumi_firmware_title),
    KUMI_DETAILS(R.string.kumi_details_title),
    KUMI_DEV(R.string.kumi_dev_title),
    IME_SETTINGS(R.string.ime_settings_title),
    SHORTCUTS(R.string.shortcuts_title),
    SETTINGS(R.string.settings_title),
}

private val SELF_HEADER_SCREENS = setOf(
    Screen.HOME,
    Screen.KUMI_AUDIO,
    Screen.KUMI_EQ,
    Screen.KUMI_KEYS,
    Screen.KUMI_FIRMWARE,
    Screen.IME_SETTINGS,
    Screen.SHORTCUTS,
)

@Composable
fun SnapApp(
    viewModel: AppViewModel = viewModel(),
    keyBus: ScreenKeyBus = remember { ScreenKeyBus() },
    deepLinkFlow: StateFlow<DeepLinkRoute?> = remember { MutableStateFlow(null) },
    onDeepLinkConsumed: () -> Unit = {},
    onExit: () -> Unit = {},
) {
    var screen by rememberSaveable { mutableStateOf(Screen.HOME) }
    var testerReturn by rememberSaveable { mutableStateOf(Screen.HOME) }
    var eqReturn by rememberSaveable { mutableStateOf(Screen.HOME) }
    var imeReturn by rememberSaveable { mutableStateOf(Screen.HOME) }
    var shortcutsReturn by rememberSaveable { mutableStateOf(Screen.HOME) }
    var eqSlot by rememberSaveable { mutableIntStateOf(0) }
    var firmwareTab by rememberSaveable { mutableIntStateOf(0) }
    var menu by remember { mutableStateOf(false) }
    val pendingDeepLink by deepLinkFlow.collectAsStateWithLifecycle()
    val openFirmware: (Int) -> Unit = { tab ->
        firmwareTab = tab
        screen = Screen.KUMI_FIRMWARE
    }
    LaunchedEffect(pendingDeepLink?.nonce) {
        val req = pendingDeepLink ?: return@LaunchedEffect
        when (req.screen) {
            "home" -> screen = Screen.HOME
            "audio" -> screen = Screen.KUMI_AUDIO
            "eq" -> {
                req.eqSlot?.let { eqSlot = it }
                eqReturn = Screen.HOME
                screen = Screen.KUMI_EQ
            }
            "keys" -> screen = Screen.KUMI_KEYS
            "ime" -> {
                imeReturn = Screen.HOME
                screen = Screen.IME_SETTINGS
            }
            "shortcuts" -> {
                shortcutsReturn = Screen.HOME
                screen = Screen.SHORTCUTS
            }
            "firmware" -> openFirmware(0)
            "settings" -> screen = Screen.SETTINGS
        }
        onDeepLinkConsumed()
    }
    val settings by viewModel.prefs.settings.collectAsStateWithLifecycle()
    val offer by viewModel.installOffer.collectAsStateWithLifecycle()
    val catalogState by viewModel.catalog.state.collectAsStateWithLifecycle()
    val appUpdate = catalogState.appUpdate(BuildConfig.VERSION_NAME)
    val home by viewModel.home.collectAsStateWithLifecycle()
    val kumiState by viewModel.kumi.state.collectAsStateWithLifecycle()
    val kind by viewModel.kumi.kind.collectAsStateWithLifecycle()
    val legacy by viewModel.legacyVisible.collectAsStateWithLifecycle()
    val streaming by viewModel.kumiStreaming.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val resources = LocalResources.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val flow by viewModel.flows.state.collectAsStateWithLifecycle()
    val busy = flow?.busy == true
    val caseKeyboardAttached = !busy && (kind == CaseKind.KUMI || kind == CaseKind.STOCK)
    val back: Screen = when (screen) {
        Screen.TECH -> if (home == HomeKind.KUMI) Screen.LEGACY else Screen.HOME
        Screen.KUMI_EQ -> eqReturn
        Screen.KUMI_DETAILS -> Screen.KUMI_FIRMWARE
        Screen.KUMI_DEV -> Screen.KUMI_DETAILS
        Screen.KEYS -> if (home == HomeKind.KUMI) Screen.KUMI_KEYS else Screen.HOME
        Screen.IME_SETTINGS -> imeReturn
        Screen.SHORTCUTS -> shortcutsReturn
        Screen.HOME -> Screen.HOME
        else -> Screen.HOME
    }

    DisposableEffect(keyBus, screen, back, busy, onExit) {
        keyBus.globalBackHandler = {
            if (!busy && screen != Screen.TESTER) {
                if (screen == Screen.HOME) {
                    onExit()
                } else {
                    screen = back
                }
                true
            } else {
                false
            }
        }
        onDispose { keyBus.globalBackHandler = null }
    }

    LaunchedEffect(busy) {
        if (busy) {
            menu = false
            keyBus.helpVisible = false
        }
    }
    LaunchedEffect(screen) {
        keyBus.helpVisible = false
    }
    LaunchedEffect(lifecycleOwner) {
        lifecycleOwner.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                viewModel.refreshNow()
                delay(POLL_MS)
            }
        }
    }
    LaunchedEffect(lifecycleOwner, home, screen, streaming) {
        if (home != HomeKind.KUMI || !streaming || (screen != Screen.HOME && screen != Screen.KUMI_AUDIO && screen != Screen.KUMI_EQ)) {
            return@LaunchedEffect
        }
        lifecycleOwner.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                val started = SystemClock.elapsedRealtime()
                viewModel.kumi.pollMetersNow()
                val elapsed = SystemClock.elapsedRealtime() - started
                delay((METER_POLL_MS - elapsed).coerceAtLeast(8L))
            }
        }
    }
    LaunchedEffect(Unit) {
        viewModel.stock.messages.collect { message -> snackbar.showSnackbar(resources.getString(messageText(message))) }
    }
    LaunchedEffect(legacy, screen) {
        if (!legacy && screen in LEGACY_SCREENS) screen = Screen.HOME
    }

    if (screen == Screen.TESTER) {
        DisposableEffect(Unit) {
            viewModel.kumi.setKeyEventsSubscribed(true)
            onDispose { viewModel.kumi.setKeyEventsSubscribed(false) }
        }
        BackHandler { screen = testerReturn }
        KeyTesterScreen(onClose = { screen = testerReturn })
        return
    }
    val view = LocalView.current
    DisposableEffect(busy) {
        view.keepScreenOn = busy
        onDispose { view.keepScreenOn = false }
    }
    if (busy) {
        BackHandler { }
    } else if (keyBus.helpVisible) {
        BackHandler { keyBus.helpVisible = false }
    } else if (screen != Screen.HOME) {
        BackHandler { screen = back }
    }

    val overflowMenu: @Composable () -> Unit = {
        if (!busy) {
            Box {
                IconButton(onClick = { menu = true }, modifier = Modifier.size(38.dp)) {
                    Icon(
                        painterResource(R.drawable.ic_more),
                        contentDescription = stringResource(R.string.action_more),
                        modifier = Modifier.size(22.dp),
                    )
                }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    if (home != HomeKind.KUMI && screen != Screen.KUMI_FIRMWARE) {
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.kumi_firmware_title)) },
                            onClick = {
                                menu = false
                                openFirmware(0)
                            },
                        )
                    }
                    if (legacy && home == HomeKind.KUMI) {
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.legacy_title)) },
                            onClick = {
                                menu = false
                                screen = Screen.LEGACY
                            },
                        )
                    }
                    if (legacy && (home == HomeKind.STOCK || screen == Screen.LEGACY)) {
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.menu_restore)) },
                            trailingIcon = { Checkbox(checked = settings.restoreOnConnect, onCheckedChange = null) },
                            onClick = { viewModel.prefs.setRestoreOnConnect(!settings.restoreOnConnect) },
                        )
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.tech_title)) },
                            onClick = {
                                menu = false
                                screen = Screen.TECH
                            },
                        )
                    }
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.menu_demo)) },
                        trailingIcon = { Checkbox(checked = settings.demoMode, onCheckedChange = null) },
                        onClick = {
                            menu = false
                            viewModel.prefs.setDemoMode(!settings.demoMode)
                        },
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.settings_title)) },
                        onClick = {
                            menu = false
                            screen = Screen.SETTINGS
                        },
                    )
                }
            }
        }
    }

    CompositionLocalProvider(
        LocalCaseKeyboard provides caseKeyboardAttached,
        LocalScreenKeyBus provides keyBus,
    ) {
        Scaffold(
            contentWindowInsets = WindowInsets(0, 0, 0, 0),
            topBar = {
                if (screen !in SELF_HEADER_SCREENS) {
                    InlineTitleHeader(
                        title = stringResource(screen.title),
                        onBack = if (!busy) ({ screen = back }) else null,
                        trailing = if (screen == Screen.LEGACY) overflowMenu else null,
                    )
                }
            },
            snackbarHost = { SnackbarHost(snackbar) },
        ) { padding ->
            Box(Modifier.padding(padding).navigationBarsPadding().fillMaxSize()) {
                val stockHome: @Composable () -> Unit = {
                    HomeScreen(
                        stock = viewModel.stock,
                        prefs = viewModel.prefs,
                        offer = offer,
                        appUpdate = appUpdate,
                        onOpenKeys = { screen = Screen.KEYS },
                        onOpenIme = {
                            imeReturn = screen
                            screen = Screen.IME_SETTINGS
                        },
                        onOpenShortcuts = {
                            shortcutsReturn = screen
                            screen = Screen.SHORTCUTS
                        },
                        onOpenTester = {
                            testerReturn = screen
                            screen = Screen.TESTER
                        },
                        onOpenFirmware = { openFirmware(0) },
                        onOpenUpdates = { openFirmware(1) },
                        showHeader = screen == Screen.HOME,
                        overflowMenu = overflowMenu,
                    )
                }
                when (screen) {
                    Screen.HOME -> if (home == HomeKind.KUMI) {
                        KumiHomeScreen(
                            kumi = viewModel.kumi,
                            catalog = viewModel.catalog.state,
                            math = viewModel.eqMath,
                            onOpenAudio = { screen = Screen.KUMI_AUDIO },
                            onOpenEq = { slot ->
                                eqSlot = slot
                                eqReturn = Screen.HOME
                                screen = Screen.KUMI_EQ
                            },
                            onOpenKeys = { screen = Screen.KUMI_KEYS },
                            onOpenIme = {
                                imeReturn = Screen.HOME
                                screen = Screen.IME_SETTINGS
                            },
                            onOpenShortcuts = {
                                shortcutsReturn = Screen.HOME
                                screen = Screen.SHORTCUTS
                            },
                            onOpenFirmware = { openFirmware(0) },
                            onOpenUpdates = { openFirmware(1) },
                            onEnableDemo = { viewModel.prefs.setDemoMode(true) },
                            overflowMenu = overflowMenu,
                        )
                    } else {
                        stockHome()
                    }
                    Screen.KUMI_AUDIO -> KumiAudioScreen(
                        kumi = viewModel.kumi,
                        math = viewModel.eqMath,
                        onBack = { screen = Screen.HOME },
                        onOpenEq = { slot ->
                            eqSlot = slot
                            eqReturn = Screen.KUMI_AUDIO
                            screen = Screen.KUMI_EQ
                        },
                        onLineOutToggled = viewModel::applyLineOutVolume,
                    )
                    Screen.LEGACY -> stockHome()
                    Screen.KEYS -> KeysScreen(viewModel.prefs)
                    Screen.TECH -> TechScreen(viewModel.stock, viewModel.prefs, viewModel.phone)
                    Screen.KUMI_EQ -> KumiEqScreen(
                        kumi = viewModel.kumi,
                        prefs = viewModel.prefs,
                        math = viewModel.eqMath,
                        initialSlot = eqSlot,
                        onBack = { screen = eqReturn },
                    )
                    Screen.KUMI_KEYS -> KumiKeysScreen(
                        kumi = viewModel.kumi,
                        prefs = viewModel.prefs,
                        onBack = { screen = Screen.HOME },
                        onOpenAndroidKeys = { screen = Screen.KEYS },
                        onOpenIme = {
                            imeReturn = Screen.KUMI_KEYS
                            screen = Screen.IME_SETTINGS
                        },
                        onOpenShortcuts = {
                            shortcutsReturn = Screen.KUMI_KEYS
                            screen = Screen.SHORTCUTS
                        },
                        onOpenTester = {
                            testerReturn = screen
                            screen = Screen.TESTER
                        },
                    )
                    Screen.IME_SETTINGS -> ImeSettingsScreen(
                        prefs = viewModel.prefs,
                        onBack = { screen = imeReturn },
                        onOpenShortcuts = {
                            shortcutsReturn = Screen.IME_SETTINGS
                            screen = Screen.SHORTCUTS
                        },
                    )
                    Screen.SHORTCUTS -> SystemShortcutsScreen(
                        prefs = viewModel.prefs,
                        onBack = { screen = shortcutsReturn },
                    )
                    Screen.KUMI_FIRMWARE -> KumiFirmwareScreen(
                        kumi = viewModel.kumi,
                        catalog = viewModel.catalog,
                        flows = viewModel.flows,
                        prefs = viewModel.prefs,
                        installBlocked = viewModel.installBlocked,
                        initialPage = firmwareTab,
                        onBack = { screen = Screen.HOME },
                        onOpenDetails = { screen = Screen.KUMI_DETAILS },
                        onOpenHome = { screen = Screen.HOME },
                    )
                    Screen.KUMI_DETAILS -> KumiDetailsScreen(
                        viewModel.kumi,
                        viewModel.flows,
                        onOpenDeveloper = { screen = Screen.KUMI_DEV },
                    )
                    Screen.KUMI_DEV -> KumiDeveloperScreen(viewModel.kumi)
                    Screen.SETTINGS -> AppSettingsScreen(viewModel.prefs)
                    Screen.TESTER -> Unit
                }

                if (keyBus.helpVisible && keyBus.shortcuts.isNotEmpty()) {
                    KeyboardShortcutOverlay(
                        shortcuts = keyBus.shortcuts,
                        hasScroll = keyBus.hasScroll,
                        mediaDoubleTap = settings.mediaDoubleTap,
                        onDismiss = { keyBus.helpVisible = false },
                    )
                }

                kumiState.presencePrompt?.let { prompt ->
                    AlertDialog(
                        onDismissRequest = { viewModel.kumi.dismissPresencePrompt() },
                        title = { Text(stringResource(R.string.kumi_presence_title)) },
                        text = {
                            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                                Text(
                                    stringResource(
                                        if (prompt.tier >= 2) R.string.kumi_presence_tier2_body else R.string.kumi_presence_tier1_body,
                                    ),
                                    style = MaterialTheme.typography.bodyMedium,
                                )
                                LinearProgressIndicator(Modifier.fillMaxWidth())
                            }
                        },
                        confirmButton = {},
                        dismissButton = {
                            TextButton(onClick = { viewModel.kumi.dismissPresencePrompt() }) {
                                Text(stringResource(R.string.action_cancel))
                            }
                        },
                    )
                }
            }
        }
    }
}

private val ALLOWED_DEEP_LINKS = setOf("home", "audio", "eq", "keys", "firmware", "ime", "shortcuts", "settings")
private val LEGACY_SCREENS = setOf(Screen.LEGACY, Screen.TECH)

private const val POLL_MS = 3500L
private const val METER_POLL_MS = 33L

private fun messageText(message: UserMessage): Int = when (message) {
    UserMessage.CASE_NOT_ANSWERING -> R.string.msg_case_not_answering
    UserMessage.WRITE_FAILED -> R.string.msg_write_failed
    UserMessage.RESTORED -> R.string.msg_restored
    UserMessage.LOAD_FAILED -> R.string.msg_load_failed
}
