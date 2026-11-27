// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.drawable.Drawable
import android.os.Bundle
import android.view.KeyEvent
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.mindone.snapcase.R
import org.mindone.snapcase.data.SettingsRepository
import org.mindone.snapcase.ui.theme.SnapTheme

class QuickSearchActivity : ComponentActivity() {
    private var queryValue by mutableStateOf(TextFieldValue(""))
    private var allApps by mutableStateOf(QuickSearchBridge.cachedApps)
    private var recentPackages by mutableStateOf(QuickSearchBridge.recentPackages())
    private var selectedIndex by mutableIntStateOf(0)
    private var pendingAutoLaunch by mutableStateOf(false)
    private val consumedUpKeys = HashSet<Int>()

    private fun currentMatches(): List<LaunchableApp> =
        QuickLauncher.searchApps(allApps, queryValue.text, recentPackages)

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        overrideActivityTransition(OVERRIDE_TRANSITION_OPEN, 0, 0)
        overrideActivityTransition(OVERRIDE_TRANSITION_CLOSE, 0, 0)
        applyPendingOrIntent(intent, replace = true)
        lifecycleScope.launch(Dispatchers.IO) {
            val saved = runCatching { SettingsRepository.from(applicationContext).settings.first().quickSearchRecent }.getOrDefault(emptyList())
            QuickSearchBridge.seedRecent(saved)
            withContext(Dispatchers.Main) { recentPackages = QuickSearchBridge.recentPackages() }
        }
        setContent {
            SnapTheme {
                QuickSearchCard(
                    query = queryValue,
                    allApps = allApps,
                    recentPackages = recentPackages,
                    selectedIndex = selectedIndex,
                    readOnlyDisplay = false,
                    onAppsLoaded = { loaded ->
                        allApps = loaded
                        QuickSearchBridge.cachedApps = loaded
                        if (pendingAutoLaunch) {
                            pendingAutoLaunch = false
                            launchAt(0)
                        }
                    },
                    onQueryChange = { next ->
                        queryValue = next
                        selectedIndex = 0
                    },
                    onLaunch = { app -> launchApp(app) },
                    onLaunchSelected = { launchAt(selectedIndex) },
                    onDismiss = { dismissNow() },
                    loadIcon = { pkg -> loadAppIcon(this@QuickSearchActivity, pkg) },
                )
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        applyPendingOrIntent(intent, replace = false)
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        QuickSearchBridge.setSessionActive(hasFocus)
        if (hasFocus) {
            drainPending()
        }
    }

    override fun onPause() {
        QuickSearchBridge.setSessionActive(false)
        super.onPause()
    }

    override fun onStop() {
        QuickSearchBridge.setSessionActive(false)
        if (!isFinishing) finish()
        super.onStop()
    }

    override fun onDestroy() {
        QuickSearchBridge.setSessionActive(false)
        super.onDestroy()
    }

    private fun applyPendingOrIntent(intent: Intent?, replace: Boolean) {
        val pending = QuickSearchBridge.consumePending()
        if (pending.dismissRequested) {
            dismissNow()
            return
        }
        val initial = pending.text.ifEmpty { intent?.getStringExtra(EXTRA_INITIAL_TEXT).orEmpty() }
        if (replace) {
            val trimmed = if (pending.backspaces > 0) initial.dropLast(pending.backspaces) else initial
            queryValue = TextFieldValue(trimmed, TextRange(trimmed.length))
            selectedIndex = 0
        } else if (initial.isNotEmpty() || pending.backspaces > 0) {
            var merged = queryValue.text
            if (pending.backspaces > 0) merged = merged.dropLast(pending.backspaces)
            merged += initial
            queryValue = TextFieldValue(merged, TextRange(merged.length))
            selectedIndex = 0
        }
        recentPackages = QuickSearchBridge.recentPackages()
        if (pending.launchTopRequested) {
            if (allApps.isNotEmpty()) {
                launchAt(0)
            } else {
                pendingAutoLaunch = true
            }
        }
    }

    private fun drainPending() {
        val pending = QuickSearchBridge.consumePending()
        if (pending.dismissRequested) {
            dismissNow()
            return
        }
        if (pending.text.isNotEmpty() || pending.backspaces > 0) {
            var merged = queryValue.text
            if (pending.backspaces > 0) merged = merged.dropLast(pending.backspaces)
            merged += pending.text
            queryValue = TextFieldValue(merged, TextRange(merged.length))
            selectedIndex = 0
        }
        if (pending.launchTopRequested) {
            if (allApps.isNotEmpty()) {
                launchAt(0)
            } else {
                pendingAutoLaunch = true
            }
        }
    }

    private fun moveSelection(delta: Int) {
        val matches = currentMatches()
        if (matches.isEmpty()) return
        val next = (selectedIndex + delta).coerceIn(0, matches.lastIndex)
        selectedIndex = next
    }

    private fun launchAt(index: Int): Boolean {
        val matches = currentMatches()
        val app = matches.getOrNull(index) ?: return false
        return launchApp(app)
    }

    private fun launchApp(app: LaunchableApp): Boolean {
        val ok = QuickLauncher.launch(this, app)
        if (ok) {
            QuickSearchBridge.recordLaunch(app.packageName)
            lifecycleScope.launch(Dispatchers.IO) {
                runCatching { SettingsRepository.from(applicationContext).recordQuickSearchRecent(app.packageName) }
            }
            dismissNow()
        }
        return ok
    }

    private fun dismissNow() {
        QuickSearchBridge.setSessionActive(false)
        finish()
    }

    @SuppressLint("RestrictedApi")
    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        val code = event.keyCode
        if (event.action == KeyEvent.ACTION_UP) {
            if (consumedUpKeys.remove(code)) return true
            return super.dispatchKeyEvent(event)
        }
        if (event.action == KeyEvent.ACTION_DOWN) {
            when (code) {
                KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> {
                    if (event.repeatCount == 0) {
                        consumedUpKeys.add(code)
                        launchAt(selectedIndex)
                    }
                    return true
                }
                KeyEvent.KEYCODE_DPAD_DOWN -> {
                    consumedUpKeys.add(code)
                    moveSelection(1)
                    return true
                }
                KeyEvent.KEYCODE_DPAD_UP -> {
                    consumedUpKeys.add(code)
                    moveSelection(-1)
                    return true
                }
                KeyEvent.KEYCODE_TAB -> {
                    consumedUpKeys.add(code)
                    moveSelection(if (event.isShiftPressed) -1 else 1)
                    return true
                }
                KeyEvent.KEYCODE_DEL -> {
                    if (queryValue.text.isEmpty() && event.repeatCount == 0) {
                        consumedUpKeys.add(code)
                        dismissNow()
                        return true
                    }
                }
                KeyEvent.KEYCODE_ESCAPE -> {
                    if (event.repeatCount == 0) {
                        consumedUpKeys.add(code)
                        dismissNow()
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
                if (quickIdx >= 0 && launchAt(quickIdx)) {
                    consumedUpKeys.add(code)
                    return true
                }
            }
        }
        return super.dispatchKeyEvent(event)
    }

    companion object {
        private const val EXTRA_INITIAL_TEXT = "initial_text"
        private const val ICON_PX = 96

        fun launch(context: Context, initialChar: Char?): Boolean {
            QuickSearchBridge.beginLaunch(initialChar)
            val intent = Intent(context, QuickSearchActivity::class.java).apply {
                addFlags(
                    Intent.FLAG_ACTIVITY_NEW_TASK or
                        Intent.FLAG_ACTIVITY_SINGLE_TOP or
                        Intent.FLAG_ACTIVITY_NO_ANIMATION,
                )
                if (initialChar != null) {
                    putExtra(EXTRA_INITIAL_TEXT, initialChar.toString())
                }
            }
            return runCatching {
                context.startActivity(intent)
                true
            }.getOrDefault(false)
        }

        fun loadAppIcon(context: Context, packageName: String): ImageBitmap? {
            QuickSearchBridge.getIcon(packageName)?.let { return it }
            val drawable = runCatching { context.packageManager.getApplicationIcon(packageName) }.getOrNull() ?: return null
            val bmp = drawableToImageBitmap(drawable, ICON_PX) ?: return null
            QuickSearchBridge.putIcon(packageName, bmp)
            return bmp
        }

        private fun drawableToImageBitmap(drawable: Drawable, sizePx: Int): ImageBitmap? = runCatching {
            val bitmap = Bitmap.createBitmap(sizePx, sizePx, Bitmap.Config.ARGB_8888)
            val canvas = Canvas(bitmap)
            drawable.setBounds(0, 0, sizePx, sizePx)
            drawable.draw(canvas)
            bitmap.asImageBitmap()
        }.getOrNull()
    }
}

@Composable
fun QuickSearchCard(
    query: TextFieldValue,
    allApps: List<LaunchableApp>,
    recentPackages: List<String>,
    selectedIndex: Int,
    readOnlyDisplay: Boolean,
    extraTopPadding: Dp = 0.dp,
    onAppsLoaded: (List<LaunchableApp>) -> Unit,
    onQueryChange: (TextFieldValue) -> Unit,
    onLaunch: (LaunchableApp) -> Unit,
    onLaunchSelected: () -> Unit,
    onDismiss: () -> Unit,
    loadIcon: (String) -> ImageBitmap?,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val focusRequester = remember { FocusRequester() }
    val listState = rememberLazyListState()
    val icons = remember { mutableStateMapOf<String, ImageBitmap>() }

    if (!readOnlyDisplay) {
        BackHandler(onBack = onDismiss)
    }

    LaunchedEffect(Unit) {
        if (!readOnlyDisplay) {
            runCatching { focusRequester.requestFocus() }
        }
        val loaded = withContext(Dispatchers.IO) { QuickLauncher.listLaunchableApps(context) }
        onAppsLoaded(loaded)
    }

    val matches = remember(allApps, query.text, recentPackages) {
        QuickLauncher.searchApps(allApps, query.text, recentPackages)
    }
    val safeSelected = selectedIndex.coerceIn(0, (matches.size - 1).coerceAtLeast(0))
    val duplicateLabels = remember(matches) {
        matches.groupingBy { it.label.lowercase() }.eachCount().filterValues { it > 1 }.keys
    }

    LaunchedEffect(safeSelected, matches.size) {
        if (matches.isNotEmpty() && safeSelected in matches.indices) {
            listState.animateScrollToItem(safeSelected)
        }
    }

    val scrimColor = if (readOnlyDisplay) Color.Black.copy(alpha = 0.42f) else Color.Transparent

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(scrimColor)
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClick = onDismiss,
            ),
        contentAlignment = Alignment.TopCenter,
    ) {
        Surface(
            modifier = Modifier
                .statusBarsPadding()
                .padding(top = extraTopPadding)
                .imePadding()
                .padding(horizontal = 12.dp, vertical = 14.dp)
                .fillMaxWidth()
                .clickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                    onClick = {},
                ),
            shape = RoundedCornerShape(22.dp),
            color = MaterialTheme.colorScheme.surfaceContainerHigh,
            tonalElevation = 6.dp,
            shadowElevation = 12.dp,
        ) {
            Column(Modifier.fillMaxWidth()) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(start = 14.dp, end = 6.dp, top = 8.dp, bottom = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Icon(
                        painterResource(R.drawable.ic_search),
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.size(20.dp),
                    )
                    Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
                        if (readOnlyDisplay) {
                            ReadOnlySearchField(query.text)
                        } else {
                            if (query.text.isEmpty()) {
                                Text(
                                    text = stringResource(R.string.home_search_placeholder),
                                    style = MaterialTheme.typography.titleMedium,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                            BasicTextField(
                                value = query,
                                onValueChange = onQueryChange,
                                singleLine = true,
                                textStyle = MaterialTheme.typography.titleMedium.copy(
                                    color = MaterialTheme.colorScheme.onSurface,
                                ),
                                cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                                keyboardOptions = KeyboardOptions(
                                    autoCorrectEnabled = false,
                                    keyboardType = KeyboardType.Uri,
                                    imeAction = ImeAction.Search,
                                ),
                                keyboardActions = KeyboardActions(
                                    onSearch = { onLaunchSelected() },
                                    onGo = { onLaunchSelected() },
                                    onDone = { onLaunchSelected() },
                                ),
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .focusRequester(focusRequester),
                            )
                        }
                    }
                    if (query.text.isNotEmpty()) {
                        IconButton(
                            onClick = { onQueryChange(TextFieldValue("")) },
                            modifier = Modifier.size(34.dp),
                        ) {
                            Text(
                                text = "×",
                                style = MaterialTheme.typography.titleMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f))
                if (matches.isEmpty()) {
                    Box(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 16.dp, vertical = 20.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            text = stringResource(R.string.home_search_empty),
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                } else {
                    LazyColumn(
                        state = listState,
                        modifier = Modifier
                            .fillMaxWidth()
                            .heightIn(max = 280.dp),
                        contentPadding = PaddingValues(horizontal = 8.dp, vertical = 6.dp),
                        verticalArrangement = Arrangement.spacedBy(2.dp),
                    ) {
                        itemsIndexed(matches, key = { _, app -> "${app.packageName}/${app.activityName}" }) { index, app ->
                            val selected = index == safeSelected
                            val showPackage = app.label.lowercase() in duplicateLabels
                            val icon = icons[app.packageName] ?: QuickSearchBridge.getIcon(app.packageName)
                            LaunchedEffect(app.packageName) {
                                if (app.packageName !in icons) {
                                    val bmp = QuickSearchBridge.getIcon(app.packageName) ?: withContext(Dispatchers.IO) {
                                        loadIcon(app.packageName)
                                    }
                                    if (bmp != null) icons[app.packageName] = bmp
                                }
                            }
                            QuickSearchResultRow(
                                app = app,
                                icon = icon,
                                selected = selected,
                                showPackage = showPackage,
                                onClick = { onLaunch(app) },
                            )
                        }
                    }
                }
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.35f))
                Text(
                    text = stringResource(R.string.home_search_footer_hint),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(horizontal = 14.dp, vertical = 7.dp),
                )
            }
        }
    }
}

@Composable
private fun ReadOnlySearchField(text: String) {
    val transition = rememberInfiniteTransition(label = "cursor")
    val cursorAlpha by transition.animateFloat(
        initialValue = 1f,
        targetValue = 0f,
        animationSpec = infiniteRepeatable(tween(520), RepeatMode.Reverse),
        label = "cursorAlpha",
    )
    Row(verticalAlignment = Alignment.CenterVertically) {
        if (text.isEmpty()) {
            Text(
                text = stringResource(R.string.home_search_placeholder),
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        } else {
            Text(
                text = text,
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Box(
                Modifier
                    .padding(start = 1.dp)
                    .width(2.dp)
                    .height(20.dp)
                    .alpha(cursorAlpha)
                    .background(MaterialTheme.colorScheme.primary),
            )
        }
    }
}

@Composable
private fun QuickSearchResultRow(
    app: LaunchableApp,
    icon: ImageBitmap?,
    selected: Boolean,
    showPackage: Boolean,
    onClick: () -> Unit,
) {
    val bg = if (selected) MaterialTheme.colorScheme.primaryContainer else Color.Transparent
    val fg = if (selected) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurface
    val subFg = if (selected) {
        MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.78f)
    } else {
        MaterialTheme.colorScheme.onSurfaceVariant
    }

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(bg)
            .clickable(onClick = onClick)
            .padding(horizontal = 10.dp, vertical = 7.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (icon != null) {
            Image(
                bitmap = icon,
                contentDescription = null,
                modifier = Modifier
                    .size(30.dp)
                    .clip(RoundedCornerShape(8.dp)),
            )
        } else {
            Box(
                modifier = Modifier
                    .size(30.dp)
                    .clip(CircleShape)
                    .background(MaterialTheme.colorScheme.surfaceContainerHighest),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = app.label.firstOrNull()?.uppercaseChar()?.toString() ?: "?",
                    style = MaterialTheme.typography.labelLarge,
                    fontWeight = FontWeight.SemiBold,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Column(Modifier.weight(1f)) {
            Text(
                text = app.label,
                style = MaterialTheme.typography.bodyLarge,
                fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal,
                color = fg,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (showPackage) {
                Text(
                    text = app.packageName,
                    style = MaterialTheme.typography.labelSmall,
                    color = subFg,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        if (selected) {
            Surface(
                shape = RoundedCornerShape(8.dp),
                color = MaterialTheme.colorScheme.primary,
                contentColor = MaterialTheme.colorScheme.onPrimary,
            ) {
                Text(
                    text = "↵",
                    style = MaterialTheme.typography.labelMedium,
                    fontWeight = FontWeight.Bold,
                    modifier = Modifier.padding(horizontal = 7.dp, vertical = 2.dp),
                )
            }
        }
    }
}
