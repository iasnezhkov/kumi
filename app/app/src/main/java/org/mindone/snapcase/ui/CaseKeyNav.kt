// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.view.KeyEvent
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.mindone.snapcase.R

class ScreenKeyBus {
    var screenHandler: ((Int, Boolean) -> Boolean)? = null
    var globalBackHandler: (() -> Boolean)? = null
    var shortcuts: List<Pair<String, String>> by mutableStateOf(emptyList())
    var helpVisible: Boolean by mutableStateOf(false)
    private var suppressCount: Int = 0
    private val consumedDown = mutableSetOf<Int>()

    fun pushSuppress() {
        suppressCount++
    }

    fun popSuppress() {
        suppressCount = (suppressCount - 1).coerceAtLeast(0)
    }

    fun dispatch(event: KeyEvent, textFocused: Boolean): Boolean {
        if (event.deviceId <= 0 || event.device?.isVirtual == true) return false
        val code = event.keyCode
        if (event.action == KeyEvent.ACTION_UP) {
            return consumedDown.remove(code)
        }
        if (event.action != KeyEvent.ACTION_DOWN) return false
        if (suppressCount > 0 || textFocused) return false
        if (KeyEvent.isModifierKey(code)) return false
        if (event.isCtrlPressed || event.isMetaPressed || event.isAltPressed) return false

        val repeat = event.repeatCount > 0
        if (helpVisible) {
            if (!repeat) {
                helpVisible = false
                consumedDown.add(code)
            }
            return true
        }
        if (!repeat && event.isShiftPressed && (code == KeyEvent.KEYCODE_SLASH || code == KeyEvent.KEYCODE_M)) {
            if (shortcuts.isNotEmpty()) {
                helpVisible = true
                consumedDown.add(code)
                return true
            }
        }
        if (event.isShiftPressed) return false
        if (!repeat && (code == KeyEvent.KEYCODE_DEL || code == KeyEvent.KEYCODE_ESCAPE)) {
            if (globalBackHandler?.invoke() == true) {
                consumedDown.add(code)
                return true
            }
        }
        if (screenHandler?.invoke(code, repeat) == true) {
            consumedDown.add(code)
            return true
        }
        return false
    }
}

val LocalCaseKeyboard = compositionLocalOf { false }
val LocalScreenKeyBus = compositionLocalOf<ScreenKeyBus?> { null }

val TOP_ROW_KEYCODES = intArrayOf(
    KeyEvent.KEYCODE_Q,
    KeyEvent.KEYCODE_W,
    KeyEvent.KEYCODE_E,
    KeyEvent.KEYCODE_R,
    KeyEvent.KEYCODE_T,
    KeyEvent.KEYCODE_Y,
    KeyEvent.KEYCODE_U,
    KeyEvent.KEYCODE_I,
    KeyEvent.KEYCODE_O,
    KeyEvent.KEYCODE_P,
)

val PRESET_GRID_KEYCODES = intArrayOf(
    KeyEvent.KEYCODE_Q,
    KeyEvent.KEYCODE_W,
    KeyEvent.KEYCODE_E,
    KeyEvent.KEYCODE_R,
    KeyEvent.KEYCODE_A,
    KeyEvent.KEYCODE_S,
    KeyEvent.KEYCODE_D,
    KeyEvent.KEYCODE_F,
)

val PRESET_GRID_LABELS = listOf("Q", "W", "E", "R", "A", "S", "D", "F")
val TOP_ROW_LABELS = listOf("Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P")

@Composable
fun RegisterScreenKeys(
    key: Any,
    shortcuts: List<Pair<String, String>>,
    onKey: (Int, Boolean) -> Boolean,
) {
    val bus = LocalScreenKeyBus.current ?: return
    DisposableEffect(bus, key, shortcuts, onKey) {
        bus.screenHandler = onKey
        bus.shortcuts = shortcuts
        onDispose {
            if (bus.screenHandler === onKey) bus.screenHandler = null
            if (bus.shortcuts === shortcuts) bus.shortcuts = emptyList()
        }
    }
}

@Composable
fun SuppressScreenKeys() {
    val bus = LocalScreenKeyBus.current ?: return
    DisposableEffect(bus) {
        bus.pushSuppress()
        onDispose { bus.popSuppress() }
    }
}

@Composable
fun KeycapBadge(
    label: String,
    modifier: Modifier = Modifier,
    emphasized: Boolean = false,
) {
    if (!LocalCaseKeyboard.current) return
    val bg = if (emphasized) {
        MaterialTheme.colorScheme.primary.copy(alpha = 0.22f)
    } else {
        MaterialTheme.colorScheme.onSurface.copy(alpha = 0.08f)
    }
    val border = if (emphasized) {
        MaterialTheme.colorScheme.primary.copy(alpha = 0.55f)
    } else {
        MaterialTheme.colorScheme.onSurface.copy(alpha = 0.24f)
    }
    val fg = if (emphasized) {
        MaterialTheme.colorScheme.primary
    } else {
        MaterialTheme.colorScheme.onSurface.copy(alpha = 0.68f)
    }
    Surface(
        color = bg,
        contentColor = fg,
        shape = RoundedCornerShape(5.dp),
        border = BorderStroke(0.85.dp, border),
        modifier = modifier.defaultMinSize(minWidth = 19.dp, minHeight = 19.dp),
    ) {
        Box(Modifier.padding(horizontal = 4.dp, vertical = 1.dp), contentAlignment = Alignment.Center) {
            Text(
                label,
                fontFamily = FontFamily.Monospace,
                fontSize = 10.sp,
                lineHeight = 12.sp,
                fontWeight = FontWeight.Bold,
                maxLines = 1,
            )
        }
    }
}

@Composable
fun InlineTabHeader(
    tabs: List<String>,
    selectedIndex: Int,
    onSelect: (Int) -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
    trailing: (@Composable () -> Unit)? = null,
) {
    Row(
        modifier
            .fillMaxWidth()
            .statusBarsPadding()
            .padding(start = 6.dp, end = 10.dp, top = 4.dp, bottom = 6.dp)
            .height(40.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        IconButton(onClick = onBack, modifier = Modifier.size(38.dp)) {
            Icon(
                painterResource(R.drawable.ic_back),
                contentDescription = stringResource(R.string.action_back),
                modifier = Modifier.size(22.dp),
            )
        }
        CompactPageTabs(
            labels = tabs,
            selectedIndex = selectedIndex,
            onSelect = onSelect,
            modifier = Modifier.weight(1f),
        )
        trailing?.invoke()
    }
}

@Composable
fun InlineTitleHeader(
    title: String,
    onBack: (() -> Unit)? = null,
    modifier: Modifier = Modifier,
    trailing: (@Composable () -> Unit)? = null,
) {
    Row(
        modifier
            .fillMaxWidth()
            .statusBarsPadding()
            .padding(start = if (onBack != null) 6.dp else 14.dp, end = 10.dp, top = 4.dp, bottom = 6.dp)
            .height(40.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        if (onBack != null) {
            IconButton(onClick = onBack, modifier = Modifier.size(38.dp)) {
                Icon(
                    painterResource(R.drawable.ic_back),
                    contentDescription = stringResource(R.string.action_back),
                    modifier = Modifier.size(22.dp),
                )
            }
        }
        Text(
            title,
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        trailing?.invoke()
    }
}

@Composable
fun KeyboardHintStrip(
    hints: List<Pair<String, String>>,
    modifier: Modifier = Modifier,
) {
    if (!LocalCaseKeyboard.current || hints.isEmpty()) return
    val bus = LocalScreenKeyBus.current
    Row(
        modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 4.dp)
            .height(26.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(
            Modifier.weight(1f),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            hints.forEach { (key, desc) ->
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    KeycapBadge(key)
                    Text(
                        desc,
                        style = MaterialTheme.typography.labelMedium,
                        fontSize = 11.5.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
        if (bus != null) {
            Surface(
                color = MaterialTheme.colorScheme.surfaceContainerHigh,
                contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
                shape = CircleShape,
                modifier = Modifier.size(24.dp).clip(CircleShape).clickable { bus.helpVisible = true },
            ) {
                Box(contentAlignment = Alignment.Center) {
                    Text(
                        "?",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Bold,
                    )
                }
            }
        }
    }
}

@Composable
fun KeyboardShortcutOverlay(
    shortcuts: List<Pair<String, String>>,
    onDismiss: () -> Unit,
) {
    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black.copy(alpha = 0.72f))
            .clickable(onClick = onDismiss)
            .padding(18.dp),
        contentAlignment = Alignment.Center,
    ) {
        Card(
            shape = RoundedCornerShape(18.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        stringResource(R.string.kumi_keys_shortcuts_title),
                        style = MaterialTheme.typography.titleMedium,
                        modifier = Modifier.weight(1f),
                    )
                    Text(
                        stringResource(R.string.kumi_keys_shortcuts_close),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                shortcuts.chunked(2).forEach { pair ->
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        pair.forEach { (key, desc) ->
                            Row(
                                Modifier.weight(1f),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(6.dp),
                            ) {
                                KeycapBadge(key, emphasized = true)
                                Text(
                                    desc,
                                    style = MaterialTheme.typography.bodySmall,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                        if (pair.size == 1) Spacer(Modifier.weight(1f))
                    }
                }
            }
        }
    }
}
