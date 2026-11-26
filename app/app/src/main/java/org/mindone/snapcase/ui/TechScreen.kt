// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import org.mindone.snapcase.R
import org.mindone.snapcase.stock.StockRepository
import org.mindone.snapcase.data.AppPreferences

@Composable
fun TechScreen(stock: StockRepository, prefs: AppPreferences, phone: PhoneState) {
    var tab by rememberSaveable { mutableIntStateOf(0) }
    val tabs = listOf(R.string.tech_tab_state, R.string.tech_tab_audio, R.string.tech_tab_settings)
    Column(Modifier.fillMaxSize()) {
        PrimaryScrollableTabRow(selectedTabIndex = tab) {
            tabs.forEachIndexed { index, title ->
                Tab(selected = tab == index, onClick = { tab = index }, text = { Text(stringResource(title)) })
            }
        }
        when (tab) {
            0 -> StatusScreen(stock, phone, onDemo = { prefs.setDemoMode(true) })
            1 -> AudioScreen(stock, prefs)
            else -> SettingsScreen(stock, prefs)
        }
    }
}
