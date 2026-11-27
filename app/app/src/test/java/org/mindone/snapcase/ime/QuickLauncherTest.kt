// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import android.view.KeyEvent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class QuickLauncherTest {
    private val sampleApps = listOf(
        LaunchableApp("org.mindone.snapcase", "Kumi", "org.mindone.snapcase.ui.MainActivity"),
        LaunchableApp("com.example.kumquat", "Kumquat Reader"),
        LaunchableApp("com.example.toolkit", "Key Toolkit"),
        LaunchableApp("com.google.android.apps.maps", "Google Maps"),
        LaunchableApp("com.google.android.youtube", "YouTube"),
        LaunchableApp("com.example.cafe", "Café Notes"),
        LaunchableApp("org.telegram.messenger", "Телеграм"),
        LaunchableApp("com.android.settings", "Settings"),
    )

    @Test
    fun prefixAndExactMatchRankKumiFirst() {
        val afterK = QuickLauncher.searchApps(sampleApps, "k")
        assertTrue(afterK.first().label.startsWith("K"))

        val afterKu = QuickLauncher.searchApps(sampleApps, "ku")
        assertEquals("org.mindone.snapcase", afterKu.first().packageName)
        assertEquals(listOf("Kumi", "Kumquat Reader"), afterKu.map { it.label })

        val exact = QuickLauncher.searchApps(sampleApps, "kumi")
        assertEquals("org.mindone.snapcase", exact.first().packageName)
    }

    @Test
    fun wordBoundaryAcronymAndDiacriticsMatch() {
        val maps = QuickLauncher.searchApps(sampleApps, "maps")
        assertEquals("com.google.android.apps.maps", maps.first().packageName)

        val acronym = QuickLauncher.searchApps(sampleApps, "gm")
        assertEquals("com.google.android.apps.maps", acronym.first().packageName)

        val camel = QuickLauncher.searchApps(sampleApps, "tube")
        assertEquals("com.google.android.youtube", camel.first().packageName)

        val cafe = QuickLauncher.searchApps(sampleApps, "cafe")
        assertEquals("com.example.cafe", cafe.first().packageName)
    }

    @Test
    fun qwertyCyrillicLayoutFallbackFindsApp() {
        val kumiTypedInRu = QuickLauncher.searchApps(sampleApps, "лгьш")
        assertEquals("org.mindone.snapcase", kumiTypedInRu.first().packageName)

        val telegramTypedInEn = QuickLauncher.searchApps(sampleApps, "ntkt")
        assertEquals("org.telegram.messenger", telegramTypedInEn.first().packageName)

        val kumiMixed = QuickLauncher.searchApps(sampleApps, "kг")
        assertEquals("org.mindone.snapcase", kumiMixed.first().packageName)

        val telegramMixed = QuickLauncher.searchApps(sampleApps, "nеле")
        assertEquals("org.telegram.messenger", telegramMixed.first().packageName)
    }

    @Test
    fun recentAppsWinTiesAndLeadEmptyQuery() {
        val recent = listOf("com.example.kumquat")
        val empty = QuickLauncher.searchApps(sampleApps, "", recentPackages = recent)
        assertEquals("com.example.kumquat", empty.first().packageName)

        val tied = QuickLauncher.searchApps(sampleApps, "ku", recentPackages = recent)
        assertEquals("com.example.kumquat", tied.first().packageName)
    }

    @Test
    fun extractSearchCharAcceptsLettersAndDigitsOnly() {
        assertEquals('k', QuickLauncher.extractSearchChar(KeyEvent.KEYCODE_K, 'k'.code, shifted = false))
        assertEquals('K', QuickLauncher.extractSearchChar(KeyEvent.KEYCODE_K, 'K'.code, shifted = true))
        assertEquals('k', QuickLauncher.extractSearchChar(KeyEvent.KEYCODE_K, 0, shifted = false))
        assertEquals('л', QuickLauncher.extractSearchChar(KeyEvent.KEYCODE_K, 'л'.code, shifted = false))
        assertEquals('5', QuickLauncher.extractSearchChar(KeyEvent.KEYCODE_5, '5'.code, shifted = false))
        assertNull(QuickLauncher.extractSearchChar(KeyEvent.KEYCODE_SPACE, ' '.code, shifted = false))
        assertNull(QuickLauncher.extractSearchChar(KeyEvent.KEYCODE_ENTER, '\n'.code, shifted = false))
        assertNull(QuickLauncher.extractSearchChar(KeyEvent.KEYCODE_DPAD_DOWN, 0, shifted = false))
        assertNull(QuickLauncher.extractSearchChar(KeyEvent.KEYCODE_ESCAPE, 0, shifted = false))
    }

    @Test
    fun homeLauncherDetectionCoversSingleAndUnsetDefault() {
        val homes = setOf("com.android.launcher3", "app.launcher", "com.teslacoilsw.launcher")
        assertTrue(QuickLauncher.isHomeLauncherPackage("com.android.launcher3", "com.android.launcher3", homes))
        assertFalse(QuickLauncher.isHomeLauncherPackage("com.android.systemui", "com.android.launcher3", homes))
        assertFalse(QuickLauncher.isHomeLauncherPackage("app.launcher", "com.android.launcher3", homes))
        assertTrue(QuickLauncher.isHomeLauncherPackage("app.launcher", "android", homes))
        assertFalse(QuickLauncher.isHomeLauncherPackage("android", "android", homes))
        assertFalse(QuickLauncher.isHomeLauncherPackage("com.android.settings", "android", homes + "com.android.settings"))
    }

    @Test
    fun handoffBuffersRapidTypingAndEnterWithoutDrops() {
        val handoff = QuickSearchHandoff(timeoutMs = 800L)
        handoff.beginLaunch('k', nowMs = 1000L)
        assertTrue(handoff.isHandoffWindow(nowMs = 1050L))
        handoff.appendChar('u')
        handoff.appendChar('x')
        handoff.backspace()
        handoff.requestLaunchTop()

        val pending = handoff.consumePending()
        assertEquals("ku", pending.text)
        assertTrue(pending.launchTopRequested)
        assertFalse(pending.dismissRequested)

        handoff.setSessionActive(true)
        assertFalse(handoff.isHandoffWindow(nowMs = 1100L))

        handoff.beginLaunch('a', nowMs = 2000L)
        assertFalse(handoff.isHandoffWindow(nowMs = 2900L))
        handoff.requestDismiss()
        assertTrue(handoff.consumePending().dismissRequested)

        handoff.beginLaunch(null, nowMs = 3000L)
        handoff.backspace()
        handoff.backspace()
        assertEquals(2, handoff.consumePending().backspaces)
    }

    @Test
    fun packageMatchingAvoidsShortNoiseWhileKeepingRomajiPackageNames() {
        val apps = listOf(
            LaunchableApp("org.mindone.snapcase", "Kumi"),
            LaunchableApp("com.kakaku.tabelog", "Tabelog"),
            LaunchableApp("jp.co.rakuten.pointclub", "楽天PointClub"),
            LaunchableApp("com.kakao.talk", "카카오톡"),
        )
        val ku = QuickLauncher.searchApps(apps, "ku")
        assertEquals(listOf("Kumi"), ku.map { it.label })

        val rakuten = QuickLauncher.searchApps(apps, "rakuten")
        assertEquals(listOf("楽天PointClub"), rakuten.map { it.label })

        val koQuery = QuickLauncher.searchApps(apps, "zkzk")
        assertEquals("com.kakao.talk", koQuery.firstOrNull()?.packageName)
    }
}
