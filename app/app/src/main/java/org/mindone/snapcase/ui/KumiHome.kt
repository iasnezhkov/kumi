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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlin.math.ln
import kotlinx.coroutines.flow.StateFlow
import org.mindone.snapcase.BuildConfig
import org.mindone.snapcase.R
import org.mindone.snapcase.kumi.AppReleaseAsset
import org.mindone.snapcase.kumi.AudioMeters
import org.mindone.snapcase.kumi.BootStages
import org.mindone.snapcase.kumi.CatalogState
import org.mindone.snapcase.kumi.CrossfeedMode
import org.mindone.snapcase.kumi.DspSettings
import org.mindone.snapcase.kumi.EqMath
import org.mindone.snapcase.kumi.EqProfile
import org.mindone.snapcase.kumi.FastEqCurve
import org.mindone.snapcase.kumi.FirmwareCatalog
import org.mindone.snapcase.kumi.Jack
import org.mindone.snapcase.kumi.KumiSession
import org.mindone.snapcase.kumi.KumiState
import org.mindone.snapcase.kumi.KumiVersion
import org.mindone.snapcase.kumi.OutputFilter
import org.mindone.snapcase.kumi.SlotState
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
    onOpenTester: () -> Unit,
    onOpenFirmware: () -> Unit,
    onOpenUpdates: () -> Unit = onOpenFirmware,
    overflowMenu: @Composable () -> Unit = {},
) {
    val kind by kumi.kind.collectAsStateWithLifecycle()
    val state by kumi.state.collectAsStateWithLifecycle()
    val firmware by catalog.collectAsStateWithLifecycle()
    val fwUpdateVersion = FirmwareCatalog.latestFirmwareUpdateVersion(
        state.device?.firmware,
        firmware.packages,
        firmware.releases,
    )?.takeIf { state.device != null && !state.ramOnly }
    val appUpdate = firmware.appUpdate(BuildConfig.VERSION_NAME)
    val hasAnyUpdate = fwUpdateVersion != null || appUpdate != null
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
    val shTester = stringResource(R.string.kb_tester_title)

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
            "T" to shTester,
        ),
    ) { code, repeat ->
        if (repeat) return@RegisterScreenKeys false
        when (code) {
            KeyEvent.KEYCODE_Q -> { onOpenAudio(); true }
            KeyEvent.KEYCODE_W -> { onOpenEq(activeEq ?: lastEqSlot); true }
            KeyEvent.KEYCODE_E -> { onOpenKeys(); true }
            KeyEvent.KEYCODE_R -> { caseOpenTarget(); true }
            KeyEvent.KEYCODE_SPACE -> if (state.connected) { toggleEq(); true } else false
            KeyEvent.KEYCODE_X -> if (state.connected && state.dsp != null) { cycleCrossfeed(); true } else false
            KeyEvent.KEYCODE_A -> if (state.connected && state.dacExtras != null) { toggleStayAwake(); true } else false
            KeyEvent.KEYCODE_S -> { onOpenIme(); true }
            KeyEvent.KEYCODE_T -> { onOpenTester(); true }
            else -> false
        }
    }

    val device = state.device
    val good = state.connected && device != null
    val stream = state.audio?.stream

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
                .verticalScroll(rememberScrollState())
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
                    shape = RoundedCornerShape(16.dp),
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh),
                ) {
                    Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text(stringResource(caseText(kind)), style = MaterialTheme.typography.bodyMedium)
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            if (kind == CaseKind.SERVICE) {
                                Button(onClick = onOpenFirmware) { Text(stringResource(R.string.kumi_open_firmware)) }
                            }
                            if (state.permissionDenied) {
                                TextButton(onClick = kumi::retry) { Text(stringResource(R.string.action_retry)) }
                            }
                        }
                    }
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
                        fwVersion = fwUpdateVersion,
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
                        streaming = stream != null,
                        defaultSlot = activeEq ?: lastEqSlot,
                        onSelectSlot = { kumi.selectEq(it) },
                        onOpenEq = onOpenEq,
                    )
                }

                val filterSummary = state.output?.let { stringResource(filterTitle(it.filter.stock())) } ?: stringResource(R.string.value_unavailable)
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
                        val updateTag = fwUpdateVersion?.toString() ?: appUpdate?.version?.toString() ?: appUpdate?.tag
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
                        icon = R.drawable.ic_check,
                        label = stringResource(R.string.kb_tester_title),
                        keyHint = "T",
                        onClick = onOpenTester,
                        modifier = Modifier.weight(1f),
                    )
                }
            }
        }

        KeyboardHintStrip(
            hints = listOf(
                "Q–R" to stringResource(R.string.kumi_key_hint_open),
                "␣" to shToggleEq,
                "X" to shXfeed,
            ),
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
    fwVersion: KumiVersion?,
    appUpdate: AppReleaseAsset?,
    onOpenUpdates: () -> Unit,
) {
    val appVer = appUpdate?.version?.toString() ?: appUpdate?.tag
    val title = when {
        appVer != null && fwVersion != null -> {
            val shown = if (appVer == fwVersion.toString()) appVer else "$appVer / $fwVersion"
            stringResource(R.string.kumi_updates_both_available, shown)
        }
        appVer != null -> stringResource(R.string.kumi_updates_app_available, appVer)
        fwVersion != null -> stringResource(R.string.kumi_update_available, fwVersion.toString())
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
        Column(Modifier.padding(11.dp), verticalArrangement = Arrangement.spacedBy(7.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(
                    stringResource(R.string.eq_title),
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                if (state.audio?.hostMuted == true) {
                    StatusPill(
                        text = stringResource(R.string.kumi_muted),
                        containerColor = MaterialTheme.colorScheme.tertiaryContainer,
                        contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
                    )
                }
                Row(horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                    SlotDotChip(stringResource(R.string.kumi_eq_off), active == null) { onSelectSlot(null) }
                    for (idx in 0 until EqProfile.SLOTS) {
                        val filled = state.eqSlots.getOrNull(idx) != null
                        SlotDotChip(stringResource(R.string.kumi_eq_slot_short, idx + 1), active == idx) {
                            if (filled) onSelectSlot(idx) else onOpenEq(idx)
                        }
                    }
                }
            }
            Row(
                Modifier.fillMaxWidth().height(72.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                ResponseCurve(shown ?: EqProfile.FLAT, math, Modifier.weight(1f).fillMaxHeight())
                HomeVerticalVuMeter(
                    liveMeters = liveMeters,
                    streaming = streaming,
                    modifier = Modifier.width(30.dp).fillMaxHeight(),
                )
            }
        }
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
        color = if (selected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
        contentColor = if (selected) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
        shape = CircleShape,
        modifier = Modifier.height(27.dp).clip(CircleShape).clickable(onClick = onClick),
    ) {
        Box(Modifier.padding(horizontal = 9.dp), contentAlignment = Alignment.Center) {
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
fun ResponseCurve(profile: EqProfile, @Suppress("UNUSED_PARAMETER") math: EqMath, modifier: Modifier = Modifier) {
    val line = MaterialTheme.colorScheme.primary
    val grid = MaterialTheme.colorScheme.outlineVariant
    val curveDb = remember(profile) { FloatArray(FastEqCurve.POINTS).also { FastEqCurve.evaluate(profile, it) } }
    val path = remember { Path() }
    val fillPath = remember { Path() }
    Canvas(modifier) {
        val range = 18.0
        val halfH = size.height / 2f
        fun y(db: Double) = (halfH - (db.coerceIn(-range, range) / range) * halfH).toFloat()
        fun x(hz: Double) = (ln(hz / 20.0) / LOG_SPAN * size.width).toFloat()
        for (db in CURVE_DB_LINES) {
            drawLine(grid, Offset(0f, y(db)), Offset(size.width, y(db)), strokeWidth = if (db == 0.0) 2f else 1f)
        }
        for (hz in CURVE_HZ_LINES) {
            drawLine(grid, Offset(x(hz), 0f), Offset(x(hz), size.height), strokeWidth = 1f)
        }
        path.reset()
        fillPath.reset()
        val zeroY = y(0.0)
        val stepX = size.width / (FastEqCurve.POINTS - 1).toFloat()
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
        drawPath(path, line, style = Stroke(width = 2.2.dp.toPx(), cap = StrokeCap.Round))
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
