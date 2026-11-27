// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.view.KeyEvent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.json.JSONObject
import org.mindone.snapcase.BuildConfig
import org.mindone.snapcase.R
import org.mindone.snapcase.data.AppPreferences
import org.mindone.snapcase.kumi.FirmwareCatalog
import org.mindone.snapcase.kumi.KumiNative

@Composable
fun AppSettingsScreen(prefs: AppPreferences) {
    val settings by prefs.settings.collectAsStateWithLifecycle()
    var repository by remember(settings.releaseRepository) { mutableStateOf(settings.releaseRepository) }
    var repoFocused by remember { mutableStateOf(false) }
    if (repoFocused) SuppressScreenKeys()
    val repoValid = repository.isBlank() || FirmwareCatalog.isValidRepository(repository)
    val bridge = remember { if (KumiNative.loaded) runCatching { JSONObject(KumiNative.info()) }.getOrNull() else null }

    val shChannel = stringResource(R.string.settings_update_channel)
    val shDemo = stringResource(R.string.menu_demo)
    val shLauncher = stringResource(R.string.settings_launcher_title)
    val shBack = stringResource(R.string.action_back)
    RegisterScreenKeys(
        key = listOf(settings.includePrereleases, settings.demoMode, settings.launcherBridgeEnabled),
        shortcuts = listOf("C" to shChannel, "L" to shLauncher, "D" to shDemo, "⌫" to shBack),
    ) { code, repeat ->
        if (repeat) return@RegisterScreenKeys false
        when (code) {
            KeyEvent.KEYCODE_C -> { prefs.setIncludePrereleases(!settings.includePrereleases); true }
            KeyEvent.KEYCODE_L -> { prefs.setLauncherBridgeEnabled(!settings.launcherBridgeEnabled); true }
            KeyEvent.KEYCODE_D -> { prefs.setDemoMode(!settings.demoMode); true }
            else -> false
        }
    }

    val listState = androidx.compose.foundation.lazy.rememberLazyListState()
    RegisterScreenScroll(listState)

    Column(Modifier.fillMaxSize()) {
        LazyColumn(
            Modifier.weight(1f).fillMaxWidth(),
            state = listState,
            contentPadding = PaddingValues(start = 12.dp, end = 12.dp, top = 4.dp, bottom = 12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
        item {
            SectionCard(stringResource(R.string.settings_legacy_title)) {
                ToggleRow(
                    title = stringResource(R.string.settings_legacy_show),
                    subtitle = stringResource(R.string.settings_legacy_text),
                    checked = settings.showLegacy,
                    onCheckedChange = { prefs.setShowLegacy(it) },
                )
            }
        }
        item {
            SectionCard(stringResource(R.string.settings_updates_title)) {
                ToggleRow(
                    title = stringResource(R.string.settings_check_releases),
                    subtitle = stringResource(R.string.settings_check_releases_text),
                    checked = settings.checkReleases,
                    onCheckedChange = { prefs.setCheckReleases(it) },
                )
                Text(stringResource(R.string.settings_update_channel), style = androidx.compose.material3.MaterialTheme.typography.labelLarge)
                CompactPageTabs(
                    labels = listOf(
                        stringResource(R.string.settings_channel_stable),
                        stringResource(R.string.settings_channel_dev),
                    ),
                    selectedIndex = if (settings.includePrereleases) 1 else 0,
                    onSelect = { prefs.setIncludePrereleases(it == 1) },
                )
                Note(
                    stringResource(
                        if (settings.includePrereleases) R.string.settings_channel_dev_desc else R.string.settings_channel_stable_desc,
                    ),
                )
                OutlinedTextField(
                    value = repository,
                    onValueChange = { repository = it.trim() },
                    label = { Text(stringResource(R.string.settings_repository)) },
                    placeholder = { Text(BuildConfig.KUMI_RELEASES_REPO) },
                    isError = !repoValid,
                    supportingText = if (!repoValid) {
                        { Text(stringResource(R.string.settings_repository_invalid)) }
                    } else {
                        null
                    },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().onFocusChanged { repoFocused = it.hasFocus },
                )
                if (repository != settings.releaseRepository && repoValid) {
                    TextButton(onClick = { prefs.setReleaseRepository(repository) }) { Text(stringResource(R.string.action_save)) }
                }
            }
        }
        item {
            SectionCard(stringResource(R.string.settings_launcher_title)) {
                ToggleRow(
                    title = stringResource(R.string.settings_launcher_bridge),
                    subtitle = stringResource(R.string.settings_launcher_bridge_text),
                    checked = settings.launcherBridgeEnabled,
                    onCheckedChange = { prefs.setLauncherBridgeEnabled(it) },
                    keyHint = "L",
                )
            }
        }
        item {
            SectionCard(stringResource(R.string.settings_demo_title)) {
                ToggleRow(
                    title = stringResource(R.string.menu_demo),
                    subtitle = stringResource(R.string.settings_demo_text),
                    checked = settings.demoMode,
                    onCheckedChange = { prefs.setDemoMode(it) },
                    keyHint = "D",
                )
            }
        }
        item {
            SectionCard(stringResource(R.string.settings_about)) {
                InfoRow(stringResource(R.string.settings_app_version), BuildConfig.VERSION_NAME)
                InfoRow(stringResource(R.string.settings_bridge), bridge?.let { "${it.optString("crate")} · protocol ${it.optInt("protocol")}" })
                InfoRow(stringResource(R.string.settings_release_key), if (BuildConfig.KUMI_RELEASE_KEY.isBlank()) stringResource(R.string.value_no) else BuildConfig.KUMI_RELEASE_KEY.take(16))
                InfoRow(stringResource(R.string.settings_license), stringResource(R.string.settings_license_value))
                Note(stringResource(R.string.settings_license_note))
            }
        }
        }
        KeyboardHintStrip(
            hints = listOf("C" to shChannel, "L" to shLauncher, "D" to shDemo, "⌫" to shBack),
        )
    }
}
