// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.view.KeyEvent
import androidx.annotation.DrawableRes
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlin.math.ln
import kotlinx.coroutines.flow.StateFlow
import org.mindone.snapcase.BuildConfig
import org.mindone.snapcase.R
import org.mindone.snapcase.kumi.AppReleaseAsset
import org.mindone.snapcase.kumi.AudioMeters
import org.mindone.snapcase.kumi.AudioSpectrum
import org.mindone.snapcase.kumi.BootStages
import org.mindone.snapcase.kumi.CatalogState
import org.mindone.snapcase.kumi.CrossfeedMode
import org.mindone.snapcase.kumi.DspSettings
import org.mindone.snapcase.kumi.EqBaseSettings
import org.mindone.snapcase.kumi.EqMath
import org.mindone.snapcase.kumi.EqProfile
import org.mindone.snapcase.kumi.FastEqCurve
import org.mindone.snapcase.kumi.FirmwareCatalog
import org.mindone.snapcase.kumi.Jack
import org.mindone.snapcase.kumi.KumiLimits
import org.mindone.snapcase.kumi.KumiSession
import org.mindone.snapcase.kumi.KumiState
import org.mindone.snapcase.kumi.KumiVersion
import org.mindone.snapcase.kumi.OutputFilter
import org.mindone.snapcase.kumi.SlotState
import org.mindone.snapcase.kumi.ToneEqSettings
import org.mindone.snapcase.protocol.DacFilter
import org.mindone.snapcase.transport.CaseKind

@Composable
fun KumiHomeScreen(
    kumi: KumiSession,
    catalog: StateFlow<CatalogState>,
    math: EqMath,
    onOpenAudio: () -> Unit,
    onOpenEq: (Int) -> Unit,
    onOpenKeys: () -> Unit,
    onOpenIme: () -> Unit,
    onOpenShortcuts: () -> Unit,
    onOpenFirmware: () -> Unit,
    onOpenUpdates: () -> Unit = onOpenFirmware,
    onEnableDemo: () -> Unit = {},
    overflowMenu: @Composable () -> Unit = {},
) {
    val kind by kumi.kind.collectAsStateWithLifecycle()
    val state by kumi.state.collectAsStateWithLifecycle()
    val firmware by catalog.collectAsStateWithLifecycle()
    val fwUpdate = FirmwareCatalog.latestFirmwareUpdate(
        state.device?.firmware,
        firmware.packages,
        firmware.releases,
        state.device?.build,
    )?.takeIf { state.device != null && !state.ramOnly }
    val appUpdate = firmware.appUpdate(BuildConfig.VERSION_NAME)
    val hasAnyUpdate = fwUpdate != null || appUpdate != null
    val caseOpenTarget: () -> Unit = if (hasAnyUpdate) onOpenUpdates else onOpenFirmware
    var lastEqSlot by rememberSaveable { mutableIntStateOf(0) }
    val activeEq = state.audio?.eqActive
    if (activeEq != null) lastEqSlot = activeEq

    val toggleEq: () -> Unit = {
        if (activeEq != null) {
            kumi.selectEq(null)
        } else {
            val target = lastEqSlot.takeIf { state.eqSlots.getOrNull(it) != null }
                ?: state.eqSlots.indexOfFirst { it != null }
            if (target >= 0) kumi.selectEq(target) else onOpenEq(0)
        }
    }
    val cycleCrossfeed: () -> Unit = {
        state.dsp?.let { current ->
            val modes = CrossfeedMode.entries
            val next = modes[(modes.indexOf(current.crossfeed) + 1) % modes.size]
            kumi.setDsp(current.copy(crossfeed = next))
        }
    }
    val toggleStayAwake: () -> Unit = {
        state.dacExtras?.let { kumi.setDacExtras(it.copy(stayAwakeWhileActive = !it.stayAwakeWhileActive)) }
    }

    val shSound = stringResource(R.string.kumi_tab_audio)
    val shEq = stringResource(R.string.eq_title)
    val shKeys = stringResource(R.string.kumi_tab_keys)
    val shCase = stringResource(R.string.kumi_tab_case)
    val shToggleEq = stringResource(R.string.kumi_key_hint_eq_ab)
    val shXfeed = stringResource(R.string.kumi_tile_crossfeed)
    val shAwake = stringResource(R.string.kumi_tile_awake)
    val shStrip = stringResource(R.string.kumi_tile_strip)
    val shShortcuts = stringResource(R.string.shortcuts_tile)

    RegisterScreenKeys(
        key = listOf(state.connected, activeEq, lastEqSlot, state.eqSlots, state.dsp, state.dacExtras),
        shortcuts = listOf(
            "Q" to shSound,
            "W" to shEq,
            "E" to shKeys,
            "R" to shCase,
            "␣" to shToggleEq,
            "X" to shXfeed,
            "A" to shAwake,
            "S" to shStrip,
            "F" to shShortcuts,
        ),
    ) { code, repeat ->
        if (repeat) return@RegisterScreenKeys false
        when (code) {
            KeyEvent.KEYCODE_Q -> if (state.connected) { onOpenAudio(); true } else false
            KeyEvent.KEYCODE_W -> if (state.connected) { onOpenEq(activeEq ?: lastEqSlot); true } else false
            KeyEvent.KEYCODE_E -> { onOpenKeys(); true }
            KeyEvent.KEYCODE_R -> { caseOpenTarget(); true }
            KeyEvent.KEYCODE_SPACE -> if (state.connected) { toggleEq(); true } else false
            KeyEvent.KEYCODE_X -> if (state.connected && state.dsp != null) { cycleCrossfeed(); true } else false
            KeyEvent.KEYCODE_A -> if (state.connected && state.dacExtras != null) { toggleStayAwake(); true } else false
            KeyEvent.KEYCODE_S -> { onOpenIme(); true }
            KeyEvent.KEYCODE_F -> { onOpenShortcuts(); true }
            else -> false
        }
    }

    val device = state.device
    val good = state.connected && device != null
    val stream = state.audio?.stream
    val scrollState = rememberScrollState()
    RegisterScreenScroll(scrollState)

    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier
                .fillMaxWidth()
                .statusBarsPadding()
                .padding(start = 14.dp, end = 6.dp, top = 4.dp, bottom = 4.dp)
                .height(40.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(7.dp),
        ) {
            Box(
                Modifier
                    .size(9.dp)
                    .clip(CircleShape)
                    .background(if (good) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline),
            )
            Text(
                if (good) stringResource(R.string.kumi_name_version, device.firmware.toString()) else stringResource(caseTitle(kind)),
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (good) {
                Text(
                    if (stream != null) streamTitle(stream.kind, stream.rateHz, stream.bits) else stringResource(R.string.kumi_case_ready),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                state.audio?.let { audio ->
                    StatusPill(
                        text = stringResource(jackShortName(audio.jack)),
                        containerColor = if (audio.jack == Jack.Empty) MaterialTheme.colorScheme.surfaceContainerHigh else MaterialTheme.colorScheme.secondaryContainer,
                        contentColor = if (audio.jack == Jack.Empty) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSecondaryContainer,
                    )
                }
            } else {
                Spacer(Modifier.weight(1f))
            }
            overflowMenu()
        }

        Column(
            Modifier
                .weight(1f)
                .fillMaxWidth()
                .verticalScroll(scrollState)
                .padding(horizontal = 12.dp, vertical = 2.dp),
            verticalArrangement = Arrangement.spacedBy(7.dp),
        ) {
            if (state.loading) LinearProgressIndicator(Modifier.fillMaxWidth())

            if (!good) {
                if (appUpdate != null) {
                    HomeUpdateCard(
                        fwVersion = null,
                        appUpdate = appUpdate,
                        onOpenUpdates = onOpenUpdates,
                    )
                }
                Card(
                    Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(18.dp),
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
                ) {
                    Column(
                        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 14.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        MindOneCaseIllustration(animated = kind == CaseKind.NONE)
                        Text(
                            stringResource(caseTitle(kind)),
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.SemiBold,
                        )
                        Text(
                            stringResource(if (state.permissionDenied) R.string.kumi_case_absent_text else caseText(kind)),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            textAlign = TextAlign.Center,
                        )
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            if (kind == CaseKind.SERVICE || kind == CaseKind.STOCK) {
                                Button(onClick = onOpenFirmware) { Text(stringResource(R.string.kumi_open_firmware)) }
                            } else {
                                FilledTonalButton(onClick = onEnableDemo) { Text(stringResource(R.string.action_try_demo)) }
                            }
                            if (state.permissionDenied) {
                                TextButton(onClick = kumi::retry) { Text(stringResource(R.string.action_retry)) }
                            }
                        }
                    }
                }
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    CompactShortcutButton(
                        icon = R.drawable.ic_keyboard,
                        label = stringResource(R.string.kumi_tile_strip),
                        keyHint = "S",
                        onClick = onOpenIme,
                        modifier = Modifier.weight(1f),
                    )
                    CompactShortcutButton(
                        icon = R.drawable.ic_search,
                        label = stringResource(R.string.shortcuts_tile),
                        keyHint = "F",
                        onClick = onOpenShortcuts,
                        modifier = Modifier.weight(1f),
                    )
                }
            } else {
                if (state.error != null || state.diagnostics?.safeMode == true || state.ramOnly) {
                    HomeAlertBanner(
                        state = state,
                        onDismiss = kumi::clearError,
                        onRestart = kumi::restartCase,
                    )
                }
                if (hasAnyUpdate && state.diagnostics?.safeMode != true) {
                    HomeUpdateCard(
                        fwVersion = fwUpdate?.toString(),
                        appUpdate = appUpdate,
                        onOpenUpdates = onOpenUpdates,
                    )
                }

                val caps = device.capabilities
                val hasEq = caps.parametricEq || state.eqSlots.any { it != null } || state.output != null
                if (hasEq) {
                    HomeEqStripCard(
                        state = state,
                        math = math,
                        liveMeters = kumi.liveMeters,
                        liveSpectrum = kumi.liveSpectrum,
                        streaming = stream != null,
                        defaultSlot = activeEq ?: lastEqSlot,
                        onSelectSlot = { kumi.selectEq(it) },
                        onOpenEq = onOpenEq,
                    )
                }

                val rawFilterSummary = state.output?.let { stringResource(filterTitle(it.filter.stock())) } ?: stringResource(R.string.value_unavailable)
                val filterSummary = if (state.dacExtras?.lineOut == true) {
                    "${stringResource(R.string.kumi_line_out_badge)} · $rawFilterSummary"
                } else {
                    rawFilterSummary
                }
                val eqSummary = activeEq?.let { idx ->
                    val p = state.eqSlots.getOrNull(idx)
                    val matched = p?.let { prof -> org.mindone.snapcase.kumi.KumiEqPresets.ALL.firstOrNull { it.bands == prof.bands } }
                    if (matched != null) stringResource(presetNameRes(matched.id)) else stringResource(R.string.kumi_eq_slot, idx + 1)
                } ?: stringResource(R.string.kumi_eq_off)
                val slotTag = state.slots?.running?.slot?.name?.let { stringResource(R.string.kumi_slot, it) } ?: device.firmware.toString()

                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        HomeNavTile(
                            icon = R.drawable.ic_headphones,
                            title = stringResource(R.string.kumi_tab_audio),
                            subtitle = filterSummary,
                            keyHint = "Q",
                            highlighted = stream != null,
                            onClick = onOpenAudio,
                            modifier = Modifier.weight(1f),
                        )
                        HomeNavTile(
                            icon = R.drawable.ic_equalizer,
                            title = stringResource(R.string.eq_title),
                            subtitle = eqSummary,
                            keyHint = "W",
                            highlighted = activeEq != null,
                            onClick = { onOpenEq(activeEq ?: lastEqSlot) },
                            modifier = Modifier.weight(1f),
                        )
                    }
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        HomeNavTile(
                            icon = R.drawable.ic_keyboard,
                            title = stringResource(R.string.kumi_tab_keys),
                            subtitle = stringResource(R.string.kumi_tile_strip_sub),
                            keyHint = "E",
                            highlighted = false,
                            onClick = onOpenKeys,
                            modifier = Modifier.weight(1f),
                        )
                        val updateTag = fwUpdate?.toString() ?: appUpdate?.displayVersion
                        HomeNavTile(
                            icon = R.drawable.ic_memory,
                            title = stringResource(R.string.kumi_tab_case),
                            subtitle = if (updateTag != null) stringResource(R.string.kumi_update_to, updateTag) else slotTag,
                            keyHint = "R",
                            highlighted = hasAnyUpdate,
                            onClick = caseOpenTarget,
                            modifier = Modifier.weight(1f),
                        )
                    }
                }

                val crossfeed = state.dsp?.crossfeed ?: CrossfeedMode.Off
                val stayAwake = state.dacExtras?.stayAwakeWhileActive == true
                val awakeValue = stringResource(if (stayAwake) R.string.value_on else R.string.value_off).replaceFirstChar { it.uppercase() }
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    QuickTogglePill(
                        label = stringResource(R.string.kumi_tile_eq),
                        value = if (activeEq != null) stringResource(R.string.kumi_eq_slot_short, activeEq + 1) else stringResource(R.string.kumi_eq_off),
                        active = activeEq != null,
                        keyHint = "␣",
                        onClick = toggleEq,
                        modifier = Modifier.weight(1f),
                    )
                    QuickTogglePill(
                        label = stringResource(R.string.kumi_tile_crossfeed),
                        value = stringResource(crossfeedShortName(crossfeed)),
                        active = crossfeed != CrossfeedMode.Off,
                        keyHint = "X",
                        enabled = state.dsp != null,
                        onClick = cycleCrossfeed,
                        modifier = Modifier.weight(1f),
                    )
                    QuickTogglePill(
                        label = stringResource(R.string.kumi_tile_awake),
                        value = awakeValue,
                        active = stayAwake,
                        keyHint = "A",
                        enabled = state.dacExtras != null,
                        onClick = toggleStayAwake,
                        modifier = Modifier.weight(1f),
                    )
                }

                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    CompactShortcutButton(
                        icon = R.drawable.ic_keyboard,
                        label = stringResource(R.string.kumi_tile_strip),
                        keyHint = "S",
                        onClick = onOpenIme,
                        modifier = Modifier.weight(1f),
                    )
                    CompactShortcutButton(
                        icon = R.drawable.ic_search,
                        label = stringResource(R.string.shortcuts_tile),
                        keyHint = "F",
                        onClick = onOpenShortcuts,
                        modifier = Modifier.weight(1f),
                    )
                }
            }
        }

        KeyboardHintStrip(
            hints = if (good) {
                listOf(
                    "Q–R" to stringResource(R.string.kumi_key_hint_open),
                    "␣" to shToggleEq,
                    "X" to shXfeed,
                )
            } else {
                listOf(
                    "S" to shStrip,
                    "F" to shShortcuts,
                    "R" to shCase,
                )
            },
        )
    }
}

@Composable
private fun HomeAlertBanner(
    state: KumiState,
    onDismiss: () -> Unit,
    onRestart: () -> Unit,
) {
    Surface(
        color = if (state.error != null || state.diagnostics?.safeMode == true) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.tertiaryContainer,
        contentColor = if (state.error != null || state.diagnostics?.safeMode == true) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onTertiaryContainer,
        shape = RoundedCornerShape(14.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            val text = when {
                state.error != null -> kumiFriendlyError(state)
                state.diagnostics?.safeMode == true -> stringResource(R.string.kumi_safe_mode)
                state.ramOnly -> stringResource(R.string.kumi_ram_only_text)
                else -> ""
            }
            Text(text, style = MaterialTheme.typography.labelMedium, maxLines = 4, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            if (state.error != null) {
                Text(
                    stringResource(R.string.action_dismiss),
                    style = MaterialTheme.typography.labelMedium,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.clickable(onClick = onDismiss).padding(horizontal = 4.dp),
                )
            } else if (state.diagnostics?.safeMode == true) {
                Surface(
                    color = MaterialTheme.colorScheme.error,
                    contentColor = MaterialTheme.colorScheme.onError,
                    shape = CircleShape,
                    modifier = Modifier.clip(CircleShape).clickable(onClick = onRestart),
                ) {
                    Text(
                        stringResource(R.string.kumi_action_restart),
                        style = MaterialTheme.typography.labelSmall,
                        modifier = Modifier.padding(horizontal = 9.dp, vertical = 5.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun HomeUpdateCard(
    fwVersion: String?,
    appUpdate: AppReleaseAsset?,
    onOpenUpdates: () -> Unit,
) {
    val appVer = appUpdate?.displayVersion
    val title = when {
        appVer != null && fwVersion != null -> {
            val shown = if (appVer == fwVersion) appVer else "$appVer / $fwVersion"
            stringResource(R.string.kumi_updates_both_available, shown)
        }
        appVer != null -> stringResource(R.string.kumi_updates_app_available, appVer)
        fwVersion != null -> stringResource(R.string.kumi_update_available, fwVersion)
        else -> stringResource(R.string.kumi_updates_title)
    }
    val subtitle = when {
        appUpdate != null && fwVersion != null -> stringResource(R.string.kumi_updates_both_hint)
        appUpdate != null -> listOf(
            stringResource(R.string.kumi_updates_size_mb, appUpdate.size / (1024.0 * 1024.0)),
            stringResource(R.string.kumi_updates_banner_hint),
        ).joinToString(" · ")
        else -> stringResource(R.string.kumi_updates_fw_banner_hint)
    }
    Card(
        onClick = onOpenUpdates,
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.primaryContainer,
            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
        ),
    ) {
        Row(
            Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Icon(
                painterResource(R.drawable.ic_bolt),
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
                modifier = Modifier.size(20.dp),
            )
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(
                    title,
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    subtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.85f),
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Button(
                onClick = onOpenUpdates,
                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 6.dp),
            ) {
                Text(stringResource(R.string.kumi_updates_cta_short), style = MaterialTheme.typography.labelLarge)
            }
        }
    }
}

@Composable
private fun HomeEqStripCard(
    state: KumiState,
    math: EqMath,
    liveMeters: StateFlow<AudioMeters?>,
    liveSpectrum: StateFlow<AudioSpectrum?>,
    streaming: Boolean,
    defaultSlot: Int,
    onSelectSlot: (Int?) -> Unit,
    onOpenEq: (Int) -> Unit,
) {
    val active = state.audio?.eqActive
    val shown = active?.let { state.eqSlots.getOrNull(it) }
    Card(
        onClick = { onOpenEq(defaultSlot) },
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Box(
            Modifier
                .fillMaxWidth()
                .height(124.dp)
                .padding(horizontal = 10.dp, vertical = 8.dp),
        ) {
            Row(
                Modifier.fillMaxSize(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                ResponseCurve(
                    profile = shown ?: EqProfile.FLAT,
                    math = math,
                    modifier = Modifier.weight(1f).fillMaxHeight(),
                    liveSpectrum = liveSpectrum,
                    eqBase = state.eqBase,
                    toneEq = state.toneEq,
                )
                HomeVerticalVuMeter(
                    liveMeters = liveMeters,
                    streaming = streaming,
                    modifier = Modifier.width(30.dp).fillMaxHeight(),
                )
            }
            Row(
                Modifier
                    .fillMaxWidth()
                    .align(Alignment.TopStart)
                    .padding(end = 38.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(5.dp),
            ) {
                Surface(
                    color = MaterialTheme.colorScheme.surfaceContainerLow.copy(alpha = 0.72f),
                    shape = CircleShape,
                ) {
                    Text(
                        stringResource(R.string.eq_title),
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.padding(horizontal = 6.dp, vertical = 1.dp),
                    )
                }
                Spacer(Modifier.weight(1f))
                if (state.audio?.hostMuted == true) {
                    StatusPill(
                        text = stringResource(R.string.kumi_muted),
                        containerColor = MaterialTheme.colorScheme.tertiaryContainer.copy(alpha = 0.90f),
                        contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
                    )
                }
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    SlotDotChip(stringResource(R.string.kumi_eq_off), active == null) { onSelectSlot(null) }
                    for (idx in 0 until EqProfile.SLOTS) {
                        val filled = state.eqSlots.getOrNull(idx) != null
                        SlotDotChip(stringResource(R.string.kumi_eq_slot_short, idx + 1), active == idx) {
                            if (filled) onSelectSlot(idx) else onOpenEq(idx)
                        }
                    }
                }
            }
            HomeLoudnessBadge(
                liveSpectrum = liveSpectrum,
                autoLevelOn = state.dspExtras?.autoLevel == true,
                modifier = Modifier.align(Alignment.BottomStart).padding(bottom = 1.dp),
            )
        }
    }
}

@Composable
private fun HomeLoudnessBadge(
    liveSpectrum: StateFlow<AudioSpectrum?>,
    autoLevelOn: Boolean,
    modifier: Modifier = Modifier,
) {
    val spectrum by liveSpectrum.collectAsStateWithLifecycle()
    val s = spectrum?.takeIf { it.active } ?: return
    val lufs = if (s.lufsShortTermCenti > KumiLimits.LUFS_FLOOR_CENTI + 50) {
        "%.1f LUFS".format(s.lufsShortTermCenti / 100.0)
    } else if (s.lufsMomentaryCenti > KumiLimits.LUFS_FLOOR_CENTI + 50) {
        "%.1f LUFS".format(s.lufsMomentaryCenti / 100.0)
    } else {
        null
    }
    val tpCenti = maxOf(s.truePeakLeftCentiDb, s.truePeakRightCentiDb)
    val tp = if (tpCenti > KumiLimits.METER_FLOOR_CENTI_DB + 100) {
        "%.1f dBTP".format(tpCenti / 100.0)
    } else {
        null
    }
    val agc = if (autoLevelOn && s.autoLevelGainCentiDb != 0) {
        "AGC ${centiDb(s.autoLevelGainCentiDb)}"
    } else {
        null
    }
    val text = listOfNotNull(lufs, tp, agc).joinToString(" · ")
    if (text.isEmpty()) return
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerHighest.copy(alpha = 0.78f),
        shape = CircleShape,
        modifier = modifier,
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1,
            modifier = Modifier.padding(horizontal = 7.dp, vertical = 2.dp),
        )
    }
}

@Composable
private fun HomeVerticalVuMeter(
    liveMeters: StateFlow<AudioMeters?>,
    streaming: Boolean,
    modifier: Modifier = Modifier,
) {
    val meters by liveMeters.collectAsStateWithLifecycle()
    VerticalStereoVuMeter(meters = meters, streaming = streaming, modifier = modifier)
}

@Composable
private fun SlotDotChip(label: String, selected: Boolean, onClick: () -> Unit) {
    Surface(
        color = if (selected) MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.92f) else MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.84f),
        contentColor = if (selected) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
        shape = CircleShape,
        modifier = Modifier.height(26.dp).clip(CircleShape).clickable(onClick = onClick),
    ) {
        Box(Modifier.padding(horizontal = 8.dp), contentAlignment = Alignment.Center) {
            Text(label, style = MaterialTheme.typography.labelMedium, fontWeight = if (selected) FontWeight.Bold else FontWeight.Medium)
        }
    }
}

@Composable
private fun HomeNavTile(
    @DrawableRes icon: Int,
    title: String,
    subtitle: String,
    keyHint: String,
    highlighted: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val bg = if (highlighted) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerLow
    val fg = if (highlighted) MaterialTheme.colorScheme.onSecondaryContainer else MaterialTheme.colorScheme.onSurface
    Card(
        onClick = onClick,
        modifier = modifier.height(66.dp),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(containerColor = bg, contentColor = fg),
    ) {
        Column(
            Modifier.fillMaxSize().padding(horizontal = 12.dp, vertical = 9.dp),
            verticalArrangement = Arrangement.SpaceBetween,
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Icon(painterResource(icon), contentDescription = null, modifier = Modifier.size(20.dp))
                Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                KeycapBadge(keyHint)
            }
            Text(
                subtitle,
                style = MaterialTheme.typography.bodySmall,
                color = fg.copy(alpha = 0.78f),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun QuickTogglePill(
    label: String,
    value: String,
    active: Boolean,
    keyHint: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    val bg = if (active) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh
    val fg = if (active) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant
    Surface(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier.height(52.dp),
        shape = RoundedCornerShape(14.dp),
        color = bg,
        contentColor = fg,
    ) {
        Column(
            Modifier.fillMaxSize().padding(horizontal = 10.dp, vertical = 7.dp),
            verticalArrangement = Arrangement.SpaceBetween,
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(label, style = MaterialTheme.typography.labelMedium, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                KeycapBadge(keyHint, emphasized = active)
            }
            Text(value, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
private fun CompactShortcutButton(
    @DrawableRes icon: Int,
    label: String,
    keyHint: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        contentColor = MaterialTheme.colorScheme.onSurface,
        shape = RoundedCornerShape(14.dp),
        modifier = modifier.height(41.dp).clip(RoundedCornerShape(14.dp)).clickable(onClick = onClick),
    ) {
        Row(
            Modifier.fillMaxSize().padding(horizontal = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Icon(painterResource(icon), contentDescription = null, modifier = Modifier.size(18.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(label, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            KeycapBadge(keyHint)
        }
    }
}

private fun caseTitle(kind: CaseKind): Int = when (kind) {
    CaseKind.SERVICE -> R.string.case_service
    CaseKind.STOCK -> R.string.kumi_case_stock
    else -> R.string.kumi_case_absent
}

private fun caseText(kind: CaseKind): Int = when (kind) {
    CaseKind.SERVICE -> R.string.kumi_case_service_text
    CaseKind.STOCK -> R.string.kumi_case_stock_text
    CaseKind.KUMI -> R.string.case_checking_text
    CaseKind.NONE -> R.string.kumi_case_absent_text
}

fun slotStateName(state: SlotState): Int = when (state) {
    SlotState.Empty -> R.string.kumi_slot_empty
    SlotState.Trial -> R.string.kumi_slot_trial
    SlotState.Confirmed -> R.string.kumi_slot_confirmed
    SlotState.Invalid -> R.string.kumi_slot_invalid
    SlotState.Unknown -> R.string.kumi_slot_unknown
}

@Composable
fun LiveVuMeter(liveMeters: StateFlow<AudioMeters?>, streaming: Boolean, showCounters: Boolean = false) {
    val meters by liveMeters.collectAsStateWithLifecycle()
    val active = meters?.takeIf { streaming || it.active } ?: return
    StereoVuMeter(active)
    if (showCounters && (active.clippedSamples > 0 || active.limiterHits > 0)) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (active.clippedSamples > 0) {
                StatusPill(
                    stringResource(R.string.kumi_meters_clip, active.clippedSamples),
                    containerColor = MaterialTheme.colorScheme.errorContainer,
                    contentColor = MaterialTheme.colorScheme.onErrorContainer,
                )
            }
            if (active.limiterHits > 0) {
                StatusPill(
                    stringResource(R.string.kumi_meters_limit, active.limiterHits),
                    containerColor = MaterialTheme.colorScheme.tertiaryContainer,
                    contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
                )
            }
        }
    }
}

private val CURVE_DB_LINES = doubleArrayOf(-12.0, 0.0, 12.0)
private val CURVE_HZ_LINES = doubleArrayOf(100.0, 1_000.0, 10_000.0)
private val LOG_SPAN = ln(1000.0)

@Composable
fun rememberSmoothedSpectrum(liveSpectrum: StateFlow<AudioSpectrum?>?): Pair<FloatArray, Long> {
    val spectrum = liveSpectrum?.collectAsStateWithLifecycle()?.value
    val smooth = remember { FloatArray(KumiLimits.SPECTRUM_BANDS) }
    var tick by remember { mutableLongStateOf(0L) }
    LaunchedEffect(spectrum) {
        val raw = spectrum?.takeIf { it.active }?.bandsDbX2
        var changed = false
        for (i in 0 until KumiLimits.SPECTRUM_BANDS) {
            val target = if (raw != null && i < raw.size) (raw[i] / 160f).coerceIn(0f, 1f) else 0f
            val cur = smooth[i]
            val next = if (target >= cur) cur + (target - cur) * 0.58f else cur * 0.84f
            val clamped = if (next < 0.004f) 0f else next
            if (clamped != cur) {
                smooth[i] = clamped
                changed = true
            }
        }
        if (changed) tick++
    }
    return smooth to tick
}

fun DrawScope.drawRtaSpectrum(
    bands: FloatArray,
    barColor: Color,
    @Suppress("UNUSED_PARAMETER") capColor: Color,
    minHz: Double = 20.0,
    logSpan: Double = LOG_SPAN,
) {
    var any = false
    for (v in bands) {
        if (v > 0.01f) {
            any = true
            break
        }
    }
    if (!any) return
    val centers = AudioSpectrum.BAND_CENTERS_HZ
    val count = minOf(bands.size, centers.size)
    if (count < 2) return
    val xs = FloatArray(count + 2)
    val ys = FloatArray(count + 2)
    val usableH = size.height * 0.94f
    xs[0] = 0f
    ys[0] = size.height - (bands[0].coerceIn(0f, 1f) * usableH * 0.72f)
    for (i in 0 until count) {
        xs[i + 1] = (ln((centers[i] / minHz).coerceAtLeast(1.0)) / logSpan * size.width).toFloat().coerceIn(0f, size.width)
        ys[i + 1] = size.height - (bands[i].coerceIn(0f, 1f) * usableH)
    }
    xs[count + 1] = size.width
    ys[count + 1] = size.height - (bands[count - 1].coerceIn(0f, 1f) * usableH * 0.65f)
    val linePath = Path().apply {
        moveTo(xs[0], ys[0])
        for (i in 1 until xs.size - 1) {
            val midX = (xs[i] + xs[i + 1]) * 0.5f
            val midY = (ys[i] + ys[i + 1]) * 0.5f
            quadraticTo(xs[i], ys[i], midX, midY)
        }
        lineTo(xs[xs.size - 1], ys[ys.size - 1])
    }
    val fillPath = Path().apply {
        addPath(linePath)
        lineTo(size.width, size.height)
        lineTo(0f, size.height)
        close()
    }
    drawPath(
        path = fillPath,
        brush = Brush.verticalGradient(
            colors = listOf(barColor.copy(alpha = 0.28f), barColor.copy(alpha = 0.09f), barColor.copy(alpha = 0.01f)),
        ),
    )
    drawPath(
        path = linePath,
        color = barColor.copy(alpha = 0.74f),
        style = Stroke(width = 1.35.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round),
    )
}

@Composable
fun ResponseCurve(
    profile: EqProfile,
    @Suppress("UNUSED_PARAMETER") math: EqMath,
    modifier: Modifier = Modifier,
    liveSpectrum: StateFlow<AudioSpectrum?>? = null,
    eqBase: EqBaseSettings? = null,
    toneEq: ToneEqSettings? = null,
) {
    val line = MaterialTheme.colorScheme.primary
    val baseLine = MaterialTheme.colorScheme.secondary
    val spectrumBar = MaterialTheme.colorScheme.tertiary
    val grid = MaterialTheme.colorScheme.outlineVariant
    val (spectrumBands, spectrumTick) = rememberSmoothedSpectrum(liveSpectrum)
    val curveDb = remember(profile, eqBase, toneEq) {
        FloatArray(FastEqCurve.POINTS).also { FastEqCurve.evaluateComposite(profile, eqBase, toneEq, it) }
    }
    val baseDb = remember(eqBase) {
        if (eqBase != null && eqBase.enabled && eqBase.profile.bands.isNotEmpty()) {
            FloatArray(FastEqCurve.POINTS).also { FastEqCurve.evaluate(eqBase.profile, it) }
        } else {
            null
        }
    }
    val path = remember { Path() }
    val fillPath = remember { Path() }
    val basePath = remember { Path() }
    val dashEffect = remember { PathEffect.dashPathEffect(floatArrayOf(8f, 6f), 0f) }
    Canvas(modifier) {
        @Suppress("UNUSED_VARIABLE")
        val rev = spectrumTick
        val range = 18.0
        val halfH = size.height / 2f
        fun y(db: Double) = (halfH - (db.coerceIn(-range, range) / range) * halfH).toFloat()
        fun x(hz: Double) = (ln(hz / 20.0) / LOG_SPAN * size.width).toFloat()
        for (db in CURVE_DB_LINES) {
            drawLine(grid.copy(alpha = if (db == 0.0) 0.75f else 0.45f), Offset(0f, y(db)), Offset(size.width, y(db)), strokeWidth = if (db == 0.0) 2f else 1f)
        }
        for (hz in CURVE_HZ_LINES) {
            drawLine(grid.copy(alpha = 0.45f), Offset(x(hz), 0f), Offset(x(hz), size.height), strokeWidth = 1f)
        }
        drawRtaSpectrum(spectrumBands, spectrumBar, line)
        val zeroY = y(0.0)
        val stepX = size.width / (FastEqCurve.POINTS - 1).toFloat()
        if (baseDb != null) {
            basePath.reset()
            for (i in 0 until FastEqCurve.POINTS) {
                val px = i * stepX
                val py = y(baseDb[i].toDouble())
                if (i == 0) basePath.moveTo(px, py) else basePath.lineTo(px, py)
            }
            drawPath(
                basePath,
                baseLine.copy(alpha = 0.60f),
                style = Stroke(width = 1.4.dp.toPx(), cap = StrokeCap.Round, pathEffect = dashEffect),
            )
        }
        path.reset()
        fillPath.reset()
        for (i in 0 until FastEqCurve.POINTS) {
            val px = i * stepX
            val py = y(curveDb[i].toDouble())
            if (i == 0) {
                path.moveTo(px, py)
                fillPath.moveTo(px, zeroY)
                fillPath.lineTo(px, py)
            } else {
                path.lineTo(px, py)
                fillPath.lineTo(px, py)
            }
        }
        fillPath.lineTo(size.width, zeroY)
        fillPath.close()
        drawPath(
            fillPath,
            brush = Brush.verticalGradient(
                colors = listOf(line.copy(alpha = 0.20f), line.copy(alpha = 0.03f), line.copy(alpha = 0.16f)),
            ),
        )
        drawPath(path, line, style = Stroke(width = 2.4.dp.toPx(), cap = StrokeCap.Round))
    }
}

fun OutputFilter.stock(): DacFilter = when (this) {
    OutputFilter.FastLowLatency -> DacFilter.FAST_LOW_LATENCY
    OutputFilter.FastPhaseCompensated -> DacFilter.FAST_PHASE_COMPENSATED
    OutputFilter.SlowLowLatency -> DacFilter.SLOW_LOW_LATENCY
    OutputFilter.SlowPhaseCompensated -> DacFilter.SLOW_PHASE_COMPENSATED
    OutputFilter.NonOversampling -> DacFilter.NON_OVERSAMPLING
}

fun centiDb(value: Int): String = when {
    value > 0 -> "+%.1f dB".format(value / 100.0)
    value < 0 -> "−%.1f dB".format(-value / 100.0)
    else -> "0 dB"
}

@Composable
fun CentiDbSlider(label: String, value: Int, min: Int, max: Int, step: Int, onCommit: (Int) -> Unit) {
    var dragging by remember { mutableStateOf(false) }
    var current by remember { mutableFloatStateOf(value.toFloat()) }
    if (!dragging && current != value.toFloat()) current = value.toFloat()
    Column {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(label, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
            Text(centiDb(current.toInt()), style = MaterialTheme.typography.labelLarge)
        }
        Slider(
            value = current,
            onValueChange = {
                dragging = true
                current = (Math.round(it / step) * step).toFloat()
            },
            onValueChangeFinished = {
                dragging = false
                if (current.toInt() != value) onCommit(current.toInt())
            },
            valueRange = min.toFloat()..max.toFloat(),
            modifier = Modifier.height(34.dp),
        )
    }
}
