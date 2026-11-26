// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.data

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import org.mindone.snapcase.ime.HapticMode
import org.mindone.snapcase.ime.LongPressBehavior
import org.mindone.snapcase.ime.SpeedKeyBinding
import org.mindone.snapcase.ime.TextSnippet
import org.mindone.snapcase.kumi.CustomEqPreset
import org.mindone.snapcase.kumi.EqProfile
import org.mindone.snapcase.protocol.EqSetting
import org.mindone.snapcase.remap.RemapTable
import org.mindone.snapcase.stock.SoundStore

class AppPreferences(private val repository: SettingsRepository, private val scope: CoroutineScope) : SoundStore {
    val rawSettings: Flow<AppSettings> = repository.settings
    val settings: StateFlow<AppSettings> = repository.settings.stateIn(scope, SharingStarted.Eagerly, AppSettings())

    private fun edit(block: suspend SettingsRepository.() -> Unit): Job = scope.launch { repository.block() }

    fun setDemoMode(value: Boolean) = edit { setDemoMode(value) }
    fun setShowLegacy(value: Boolean) = edit { setShowLegacy(value) }
    fun setCheckReleases(value: Boolean) = edit { setCheckReleases(value) }
    fun setIncludePrereleases(value: Boolean) = edit { setIncludePrereleases(value) }
    fun setReleaseRepository(value: String) = edit { setReleaseRepository(value) }
    fun setRemapOnlySnap(value: Boolean) = edit { setRemapOnlySnap(value) }
    fun setEqSampleRate(value: Int) = edit { setEqSampleRate(value) }
    fun setRestoreOnConnect(value: Boolean) = edit { setRestoreOnConnect(value) }
    fun dismissOffer(version: String) = edit { setOfferDismissed(version) }
    fun addRemap(from: Int, to: Int) = edit { updateRemapTable { it.with(from, to) } }
    fun removeRemap(from: Int) = edit { updateRemapTable { it.without(from) } }
    fun clearRemap() = edit { updateRemapTable { RemapTable.EMPTY } }
    fun saveCustomEqPreset(name: String, profile: EqProfile) = edit {
        val clean = name.trim()
        if (clean.isNotEmpty() && profile.valid) {
            updateCustomEqPresets { existing -> existing.filterNot { it.name.equals(clean, ignoreCase = true) } + CustomEqPreset(clean, profile) }
        }
    }
    fun deleteCustomEqPreset(name: String) = edit {
        updateCustomEqPresets { existing -> existing.filterNot { it.name == name } }
    }
    fun setImeStripEnabled(value: Boolean) = edit { setImeStripEnabled(value) }
    fun setImeAlwaysShowStrip(value: Boolean) = edit { setImeAlwaysShowStrip(value) }
    fun setImeShowOnTypingOnly(value: Boolean) = edit { setImeShowOnTypingOnly(value) }
    fun setImeAutoCorrect(value: Boolean) = edit { setImeAutoCorrect(value) }
    fun setImeAutoCapitalize(value: Boolean) = edit { setImeAutoCapitalize(value) }
    fun setImeDoubleSpacePeriod(value: Boolean) = edit { setImeDoubleSpacePeriod(value) }
    fun setImeStickyShift(value: Boolean) = edit { setImeStickyShift(value) }
    fun setImeLongPressBehavior(value: LongPressBehavior) = edit { setImeLongPressBehavior(value) }
    fun setImeHapticMode(value: HapticMode) = edit { setImeHapticMode(value) }
    fun setImeHapticDurationMs(value: Int) = edit { setImeHapticDurationMs(value) }
    fun setImeShowQuickExtras(value: Boolean) = edit { setImeShowQuickExtras(value) }
    fun setImeDefaultLang(value: String) = edit { setImeDefaultLang(value) }
    fun setImeEnabledLangs(langs: List<String>) = edit { setImeEnabledLangs(langs) }
    fun setImeShiftSpaceSwitch(value: Boolean) = edit { setImeShiftSpaceSwitch(value) }
    fun addImeSnippet(trigger: String, expansion: String) = edit {
        val cleanTrigger = trigger.trim().lowercase()
        val cleanExpansion = expansion.trim()
        if (cleanTrigger.isNotEmpty() && cleanExpansion.isNotEmpty()) {
            updateImeSnippets { list ->
                list.filterNot { it.trigger.equals(cleanTrigger, ignoreCase = true) } + TextSnippet(cleanTrigger, cleanExpansion)
            }
        }
    }
    fun removeImeSnippet(trigger: String) = edit {
        updateImeSnippets { list -> list.filterNot { it.trigger.equals(trigger, ignoreCase = true) } }
    }
    fun setSpeedKeysEnabled(value: Boolean) = edit { setSpeedKeysEnabled(value) }
    fun setSpeedKey(binding: SpeedKeyBinding) = edit {
        if (binding.valid) {
            updateSpeedKeys { list -> list.filterNot { it.keyCode == binding.keyCode } + binding }
        }
    }
    fun removeSpeedKey(keyCode: Int) = edit {
        updateSpeedKeys { list -> list.filterNot { it.keyCode == keyCode } }
    }

    suspend fun setKumiSeen(value: Boolean) {
        repository.setKumiSeen(value)
    }

    suspend fun setStockBackup(value: String) {
        repository.setStockBackup(value)
    }

    override suspend fun saveEq(value: EqSetting) {
        repository.saveEq(value)
    }

    override suspend fun saveDacFilter(wire: Int) {
        repository.saveDacFilter(wire)
    }

    override suspend fun saveMicGain(db: Double) {
        repository.saveMicGain(db)
    }

    override suspend fun setExperimentalWrites(value: Boolean) {
        repository.setExperimentalWrites(value)
    }
}
