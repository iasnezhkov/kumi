// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import androidx.compose.foundation.layout.Arrangement
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

    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
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
                ToggleRow(
                    title = stringResource(R.string.settings_prereleases),
                    checked = settings.includePrereleases,
                    onCheckedChange = { prefs.setIncludePrereleases(it) },
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
            SectionCard(stringResource(R.string.settings_demo_title)) {
                ToggleRow(
                    title = stringResource(R.string.menu_demo),
                    subtitle = stringResource(R.string.settings_demo_text),
                    checked = settings.demoMode,
                    onCheckedChange = { prefs.setDemoMode(it) },
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
}
