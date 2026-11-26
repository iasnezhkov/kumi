// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

data class KumiLiveSnapshot(
    val connected: Boolean = false,
    val activeLayer: Int = 0,
    val capsWord: Boolean = false,
    val hostLeds: Int = 0,
)

object SnapLiveMirror {
    private val _snapshot = MutableStateFlow(KumiLiveSnapshot())
    val snapshot: StateFlow<KumiLiveSnapshot> = _snapshot.asStateFlow()

    fun update(state: KumiState) {
        val kb = state.keyboard
        _snapshot.value = KumiLiveSnapshot(
            connected = state.connected,
            activeLayer = kb?.activeLayer ?: 0,
            capsWord = kb?.capsWord == true,
            hostLeds = kb?.hostLeds ?: 0,
        )
    }

    fun clear() {
        _snapshot.value = KumiLiveSnapshot()
    }
}
