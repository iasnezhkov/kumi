// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.data

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import androidx.datastore.preferences.core.doublePreferencesKey
import org.mindone.snapcase.ime.ClipboardStore
import org.mindone.snapcase.ime.HapticMode
import org.mindone.snapcase.ime.LongPressBehavior
import org.mindone.snapcase.ime.QuickLauncher
import org.mindone.snapcase.ime.SpeedKeyBinding
import org.mindone.snapcase.ime.TextSnippet
import org.mindone.snapcase.kumi.CustomEqPreset
import org.mindone.snapcase.protocol.BiquadDesigner
import org.mindone.snapcase.protocol.EqSetting
import org.mindone.snapcase.remap.RemapTable

val Context.snapDataStore: DataStore<Preferences> by preferencesDataStore(name = "snapcase")

data class AppSettings(
    val experimentalWrites: Boolean = false,
    val demoMode: Boolean = false,
    val remapTable: RemapTable = RemapTable.EMPTY,
    val remapOnlySnap: Boolean = true,
    val eqSampleRate: Int = BiquadDesigner.DEFAULT_SAMPLE_RATE,
    val savedEq: EqSetting? = null,
    val savedDacFilter: Int? = null,
    val savedMicGainDb: Double? = null,
    val restoreOnConnect: Boolean = true,
    val showLegacy: Boolean = false,
    val checkReleases: Boolean = true,
    val includePrereleases: Boolean = false,
    val releaseRepository: String = "",
    val stockBackup: String? = null,
    val kumiSeen: Boolean = false,
    val offerDismissed: String? = null,
    val customEqPresets: List<CustomEqPreset> = emptyList(),
    val imeStripEnabled: Boolean = true,
    val imeAlwaysShowStrip: Boolean = true,
    val imeShowOnTypingOnly: Boolean = true,
    val imeAutoCorrect: Boolean = true,
    val imeAutoCapitalize: Boolean = true,
    val imeDoubleSpacePeriod: Boolean = true,
    val imeStickyShift: Boolean = false,
    val imeLongPressBehavior: LongPressBehavior = LongPressBehavior.ALT_SYMBOL,
    val imeHapticMode: HapticMode = HapticMode.EVENTS,
    val imeHapticDurationMs: Int = 5,
    val imeShowQuickExtras: Boolean = true,
    val imeDefaultLang: String = "en",
    val imeEnabledLangs: List<String> = listOf("en", "ru"),
    val imeShiftSpaceSwitch: Boolean = true,
    val imeSnippets: List<TextSnippet> = ClipboardStore.DEFAULT_SNIPPETS,
    val speedKeysEnabled: Boolean = true,
    val speedKeys: List<SpeedKeyBinding> = emptyList(),
    val homeTypeToSearch: Boolean = true,
    val backspaceAsBack: Boolean = false,
    val mediaDoubleTap: Boolean = false,
    val backspaceBackExcluded: Set<String> = SettingsRepository.DEFAULT_BACKSPACE_EXCLUDED,
    val quickSearchRecent: List<String> = emptyList(),
    val launcherBridgeEnabled: Boolean = true,
)

class SettingsRepository(private val store: DataStore<Preferences>) {

    val settings: Flow<AppSettings> = store.data.map { prefs ->
        AppSettings(
            experimentalWrites = prefs[EXPERIMENTAL_WRITES] ?: false,
            demoMode = prefs[DEMO_MODE] ?: false,
            remapTable = RemapTable.parseOrEmpty(prefs[REMAP_TABLE]),
            remapOnlySnap = prefs[REMAP_ONLY_SNAP] ?: true,
            eqSampleRate = prefs[EQ_SAMPLE_RATE] ?: BiquadDesigner.DEFAULT_SAMPLE_RATE,
            savedEq = EqSetting.parse(prefs[SAVED_EQ]),
            savedDacFilter = prefs[SAVED_DAC_FILTER],
            savedMicGainDb = prefs[SAVED_MIC_GAIN],
            restoreOnConnect = prefs[RESTORE_ON_CONNECT] ?: true,
            showLegacy = prefs[SHOW_LEGACY] ?: false,
            checkReleases = prefs[CHECK_RELEASES] ?: true,
            includePrereleases = prefs[INCLUDE_PRERELEASES] ?: false,
            releaseRepository = prefs[RELEASE_REPOSITORY] ?: "",
            stockBackup = prefs[STOCK_BACKUP],
            kumiSeen = prefs[KUMI_SEEN] ?: false,
            offerDismissed = prefs[OFFER_DISMISSED],
            customEqPresets = CustomEqPreset.parseList(prefs[CUSTOM_EQ_PRESETS]),
            imeStripEnabled = prefs[IME_STRIP_ENABLED] ?: true,
            imeAlwaysShowStrip = prefs[IME_ALWAYS_SHOW_STRIP] ?: true,
            imeShowOnTypingOnly = prefs[IME_SHOW_ON_TYPING_ONLY] ?: true,
            imeAutoCorrect = prefs[IME_AUTO_CORRECT] ?: true,
            imeAutoCapitalize = prefs[IME_AUTO_CAPITALIZE] ?: true,
            imeDoubleSpacePeriod = prefs[IME_DOUBLE_SPACE_PERIOD] ?: true,
            imeStickyShift = prefs[IME_STICKY_SHIFT] ?: false,
            imeLongPressBehavior = runCatching {
                LongPressBehavior.valueOf(prefs[IME_LONG_PRESS] ?: LongPressBehavior.ALT_SYMBOL.name)
            }.getOrDefault(LongPressBehavior.ALT_SYMBOL),
            imeHapticMode = runCatching {
                HapticMode.valueOf(prefs[IME_HAPTIC_MODE] ?: HapticMode.EVENTS.name)
            }.getOrDefault(HapticMode.EVENTS),
            imeHapticDurationMs = (prefs[IME_HAPTIC_MS] ?: 5).coerceIn(4, 15),
            imeShowQuickExtras = prefs[IME_SHOW_QUICK_EXTRAS] ?: true,
            imeDefaultLang = prefs[IME_DEFAULT_LANG] ?: "en",
            imeEnabledLangs = parseLangList(prefs[IME_ENABLED_LANGS]),
            imeShiftSpaceSwitch = prefs[IME_SHIFT_SPACE_SWITCH] ?: true,
            imeSnippets = ClipboardStore.parseSnippets(prefs[IME_SNIPPETS]),
            speedKeysEnabled = prefs[SPEED_KEYS_ENABLED] ?: true,
            speedKeys = QuickLauncher.parseBindings(prefs[SPEED_KEYS]),
            homeTypeToSearch = prefs[HOME_TYPE_TO_SEARCH] ?: true,
            backspaceAsBack = prefs[BACKSPACE_AS_BACK] ?: false,
            mediaDoubleTap = prefs[MEDIA_DOUBLE_TAP] ?: false,
            backspaceBackExcluded = parsePackageSet(prefs[BACKSPACE_BACK_EXCLUDED]),
            quickSearchRecent = parseRecentList(prefs[QUICK_SEARCH_RECENT]),
            launcherBridgeEnabled = prefs[LAUNCHER_BRIDGE_ENABLED] ?: true,
        )
    }

    suspend fun setExperimentalWrites(value: Boolean) = store.edit { it[EXPERIMENTAL_WRITES] = value }

    suspend fun setDemoMode(value: Boolean) = store.edit { it[DEMO_MODE] = value }

    suspend fun setRemapOnlySnap(value: Boolean) = store.edit { it[REMAP_ONLY_SNAP] = value }

    suspend fun setEqSampleRate(value: Int) = store.edit { it[EQ_SAMPLE_RATE] = value }

    suspend fun saveEq(value: EqSetting) = store.edit { it[SAVED_EQ] = value.serialize() }

    suspend fun saveDacFilter(value: Int) = store.edit { it[SAVED_DAC_FILTER] = value }

    suspend fun saveMicGain(value: Double) = store.edit { it[SAVED_MIC_GAIN] = value }

    suspend fun setRestoreOnConnect(value: Boolean) = store.edit { it[RESTORE_ON_CONNECT] = value }

    suspend fun setShowLegacy(value: Boolean) = store.edit { it[SHOW_LEGACY] = value }

    suspend fun setCheckReleases(value: Boolean) = store.edit { it[CHECK_RELEASES] = value }

    suspend fun setIncludePrereleases(value: Boolean) = store.edit { it[INCLUDE_PRERELEASES] = value }

    suspend fun setReleaseRepository(value: String) = store.edit { it[RELEASE_REPOSITORY] = value.trim() }

    suspend fun setStockBackup(value: String) = store.edit { it[STOCK_BACKUP] = value }

    suspend fun setKumiSeen(value: Boolean) = store.edit { it[KUMI_SEEN] = value }

    suspend fun setOfferDismissed(value: String?) = store.edit { prefs ->
        if (value == null) prefs.remove(OFFER_DISMISSED) else prefs[OFFER_DISMISSED] = value
    }

    suspend fun updateRemapTable(transform: (RemapTable) -> RemapTable) = store.edit { prefs ->
        prefs[REMAP_TABLE] = transform(RemapTable.parseOrEmpty(prefs[REMAP_TABLE])).serialize()
    }

    suspend fun updateCustomEqPresets(transform: (List<CustomEqPreset>) -> List<CustomEqPreset>) = store.edit { prefs ->
        prefs[CUSTOM_EQ_PRESETS] = CustomEqPreset.serializeList(transform(CustomEqPreset.parseList(prefs[CUSTOM_EQ_PRESETS])))
    }

    suspend fun setImeStripEnabled(value: Boolean) = store.edit { it[IME_STRIP_ENABLED] = value }
    suspend fun setImeAlwaysShowStrip(value: Boolean) = store.edit { it[IME_ALWAYS_SHOW_STRIP] = value }
    suspend fun setImeShowOnTypingOnly(value: Boolean) = store.edit { it[IME_SHOW_ON_TYPING_ONLY] = value }
    suspend fun setImeAutoCorrect(value: Boolean) = store.edit { it[IME_AUTO_CORRECT] = value }
    suspend fun setImeAutoCapitalize(value: Boolean) = store.edit { it[IME_AUTO_CAPITALIZE] = value }
    suspend fun setImeDoubleSpacePeriod(value: Boolean) = store.edit { it[IME_DOUBLE_SPACE_PERIOD] = value }
    suspend fun setImeStickyShift(value: Boolean) = store.edit { it[IME_STICKY_SHIFT] = value }
    suspend fun setImeLongPressBehavior(value: LongPressBehavior) = store.edit { it[IME_LONG_PRESS] = value.name }
    suspend fun setImeHapticMode(value: HapticMode) = store.edit { it[IME_HAPTIC_MODE] = value.name }
    suspend fun setImeHapticDurationMs(value: Int) = store.edit { it[IME_HAPTIC_MS] = value.coerceIn(4, 15) }
    suspend fun setImeShowQuickExtras(value: Boolean) = store.edit { it[IME_SHOW_QUICK_EXTRAS] = value }
    suspend fun setImeDefaultLang(value: String) = store.edit { it[IME_DEFAULT_LANG] = value }
    suspend fun setImeEnabledLangs(langs: List<String>) = store.edit {
        val clean = langs.map { l -> l.lowercase() }.distinct().ifEmpty { listOf("en") }
        it[IME_ENABLED_LANGS] = clean.joinToString(",")
    }
    suspend fun setImeShiftSpaceSwitch(value: Boolean) = store.edit { it[IME_SHIFT_SPACE_SWITCH] = value }
    suspend fun updateImeSnippets(transform: (List<TextSnippet>) -> List<TextSnippet>) = store.edit { prefs ->
        prefs[IME_SNIPPETS] = ClipboardStore.serializeSnippets(transform(ClipboardStore.parseSnippets(prefs[IME_SNIPPETS])))
    }
    suspend fun setSpeedKeysEnabled(value: Boolean) = store.edit { it[SPEED_KEYS_ENABLED] = value }
    suspend fun setHomeTypeToSearch(value: Boolean) = store.edit { it[HOME_TYPE_TO_SEARCH] = value }
    suspend fun setBackspaceAsBack(value: Boolean) = store.edit { it[BACKSPACE_AS_BACK] = value }
    suspend fun setMediaDoubleTap(value: Boolean) = store.edit { it[MEDIA_DOUBLE_TAP] = value }
    suspend fun setLauncherBridgeEnabled(value: Boolean) = store.edit { it[LAUNCHER_BRIDGE_ENABLED] = value }
    suspend fun updateBackspaceBackExcluded(transform: (Set<String>) -> Set<String>) = store.edit { prefs ->
        val next = transform(parsePackageSet(prefs[BACKSPACE_BACK_EXCLUDED]))
        prefs[BACKSPACE_BACK_EXCLUDED] = next.sorted().joinToString(",")
    }
    suspend fun recordQuickSearchRecent(packageName: String) = store.edit { prefs ->
        val clean = packageName.trim()
        if (clean.isEmpty()) return@edit
        val next = (listOf(clean) + parseRecentList(prefs[QUICK_SEARCH_RECENT]).filterNot { it == clean }).take(24)
        prefs[QUICK_SEARCH_RECENT] = next.joinToString(",")
    }
    suspend fun updateSpeedKeys(transform: (List<SpeedKeyBinding>) -> List<SpeedKeyBinding>) = store.edit { prefs ->
        prefs[SPEED_KEYS] = QuickLauncher.serializeBindings(transform(QuickLauncher.parseBindings(prefs[SPEED_KEYS])))
    }

    companion object {
        val DEFAULT_BACKSPACE_EXCLUDED: Set<String> = setOf(
            "com.termux",
            "org.connectbot",
            "com.sonelli.juicessh",
            "com.server.auditor.ssh.client",
            "com.realvnc.viewer.android",
            "com.microsoft.rdc.androidx",
            "com.google.android.apps.chromeremotedesktop",
            "com.termoneplus",
            "jackpal.androidterm",
        )

        private val EXPERIMENTAL_WRITES = booleanPreferencesKey("experimental_writes")
        private val DEMO_MODE = booleanPreferencesKey("demo_mode")
        private val REMAP_TABLE = stringPreferencesKey("remap_table")
        private val REMAP_ONLY_SNAP = booleanPreferencesKey("remap_only_snap")
        private val EQ_SAMPLE_RATE = intPreferencesKey("eq_sample_rate")
        private val SAVED_EQ = stringPreferencesKey("saved_eq")
        private val SAVED_DAC_FILTER = intPreferencesKey("saved_dac_filter")
        private val SAVED_MIC_GAIN = doublePreferencesKey("saved_mic_gain")
        private val RESTORE_ON_CONNECT = booleanPreferencesKey("restore_on_connect")
        private val SHOW_LEGACY = booleanPreferencesKey("show_legacy")
        private val CHECK_RELEASES = booleanPreferencesKey("check_releases")
        private val INCLUDE_PRERELEASES = booleanPreferencesKey("include_prereleases")
        private val RELEASE_REPOSITORY = stringPreferencesKey("release_repository")
        private val STOCK_BACKUP = stringPreferencesKey("kumi_stock_backup")
        private val KUMI_SEEN = booleanPreferencesKey("kumi_seen")
        private val OFFER_DISMISSED = stringPreferencesKey("kumi_offer_dismissed")
        private val CUSTOM_EQ_PRESETS = stringPreferencesKey("kumi_custom_eq_presets")
        private val IME_STRIP_ENABLED = booleanPreferencesKey("ime_strip_enabled")
        private val IME_ALWAYS_SHOW_STRIP = booleanPreferencesKey("ime_always_show_strip")
        private val IME_SHOW_ON_TYPING_ONLY = booleanPreferencesKey("ime_show_on_typing_only")
        private val IME_AUTO_CORRECT = booleanPreferencesKey("ime_auto_correct")
        private val IME_AUTO_CAPITALIZE = booleanPreferencesKey("ime_auto_capitalize")
        private val IME_DOUBLE_SPACE_PERIOD = booleanPreferencesKey("ime_double_space_period")
        private val IME_STICKY_SHIFT = booleanPreferencesKey("ime_sticky_shift")
        private val IME_LONG_PRESS = stringPreferencesKey("ime_long_press")
        private val IME_HAPTIC_MODE = stringPreferencesKey("ime_haptic_mode")
        private val IME_HAPTIC_MS = intPreferencesKey("ime_haptic_ms")
        private val IME_SHOW_QUICK_EXTRAS = booleanPreferencesKey("ime_show_quick_extras")
        private val IME_DEFAULT_LANG = stringPreferencesKey("ime_default_lang")
        private val IME_ENABLED_LANGS = stringPreferencesKey("ime_enabled_langs")
        private val IME_SHIFT_SPACE_SWITCH = booleanPreferencesKey("ime_shift_space_switch")
        private val IME_SNIPPETS = stringPreferencesKey("ime_snippets")
        private val SPEED_KEYS_ENABLED = booleanPreferencesKey("speed_keys_enabled")
        private val SPEED_KEYS = stringPreferencesKey("speed_keys")
        private val HOME_TYPE_TO_SEARCH = booleanPreferencesKey("home_type_to_search")
        private val BACKSPACE_AS_BACK = booleanPreferencesKey("backspace_as_back")
        private val MEDIA_DOUBLE_TAP = booleanPreferencesKey("media_double_tap")
        private val BACKSPACE_BACK_EXCLUDED = stringPreferencesKey("backspace_back_excluded")
        private val QUICK_SEARCH_RECENT = stringPreferencesKey("quick_search_recent")
        private val LAUNCHER_BRIDGE_ENABLED = booleanPreferencesKey("launcher_bridge_enabled")

        private fun parseLangList(raw: String?): List<String> {
            if (raw.isNullOrBlank()) return listOf("en", "ru")
            return raw.split(',').map { it.trim().lowercase() }.filter { it.isNotEmpty() }.distinct().ifEmpty { listOf("en", "ru") }
        }

        private fun parsePackageSet(raw: String?): Set<String> {
            if (raw == null) return DEFAULT_BACKSPACE_EXCLUDED
            return raw.split(',').map { it.trim() }.filter { it.isNotEmpty() }.toSet()
        }

        private fun parseRecentList(raw: String?): List<String> {
            if (raw.isNullOrBlank()) return emptyList()
            return raw.split(',').map { it.trim() }.filter { it.isNotEmpty() }.distinct().take(24)
        }

        fun from(context: Context) = SettingsRepository(context.applicationContext.snapDataStore)
    }
}
