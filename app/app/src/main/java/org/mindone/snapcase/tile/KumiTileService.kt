// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.tile

import android.app.PendingIntent
import android.content.Intent
import android.hardware.usb.UsbManager
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import org.mindone.snapcase.R
import org.mindone.snapcase.kumi.KumiUsb
import org.mindone.snapcase.ui.MainActivity

class KumiTileService : TileService() {
    override fun onStartListening() {
        super.onStartListening()
        val manager = getSystemService(UsbManager::class.java)
        val connected = manager?.deviceList?.values?.any { KumiUsb.isKumi(it) } == true
        qsTile?.apply {
            label = getString(R.string.kumi_tile_label)
            subtitle = getString(if (connected) R.string.case_connected else R.string.kumi_tile_subtitle)
            state = if (connected) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
            updateTile()
        }
    }

    override fun onClick() {
        super.onClick()
        val intent = Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        val pending = PendingIntent.getActivity(this, 0, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        startActivityAndCollapse(pending)
    }
}
