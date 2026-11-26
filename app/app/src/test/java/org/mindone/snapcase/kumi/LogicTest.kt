// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.mindone.snapcase.transport.CaseKind

class LogicTest {
    @Test
    fun versionsParseFromTagsAndCompare() {
        assertEquals(KumiVersion(0, 2, 10), KumiVersion.parse("v0.2.10"))
        assertEquals(KumiVersion(1, 0, 0), KumiVersion.parse("1.0.0-rc1"))
        assertNull(KumiVersion.parse("latest"))
        assertTrue(KumiVersion(0, 10, 0) > KumiVersion(0, 9, 9))
    }

    @Test
    fun releasesKeepOnlySignedPackageAssetsNewestFirst() {
        val json = """
            [
              {"tag_name":"v0.1.0","name":"First","body":"notes","draft":false,"prerelease":false,"published_at":"2026-11-01T00:00:00Z",
               "assets":[{"name":"kumi-0.1.0.kumi","size":80000,"browser_download_url":"https://github.com/o/kumi/releases/download/v0.1.0/kumi-0.1.0.kumi"},
                         {"name":"kumi-0.1.0.elf","size":80000,"browser_download_url":"https://github.com/x.elf"}]},
              {"tag_name":"v0.3.0","name":"","body":"","draft":true,"prerelease":false,"published_at":"2026-11-03T00:00:00Z",
               "assets":[{"name":"kumi-0.3.0.kumi","size":1,"browser_download_url":"https://github.com/x.kumi"}]},
              {"tag_name":"v0.2.0-beta","name":"Beta","body":"","draft":false,"prerelease":true,"published_at":"2026-11-02T00:00:00Z",
               "assets":[{"name":"kumi-0.2.0.kumi","size":81000,"browser_download_url":"https://github.com/o/kumi/b.kumi"},
                         {"name":"huge.kumi","size":99999999,"browser_download_url":"https://github.com/o/kumi/h.kumi"},
                         {"name":"plain.kumi","size":10,"browser_download_url":"http://example.com/p.kumi"}]}
            ]
        """.trimIndent()
        val stable = ReleaseAsset.parseReleases(json, includePrereleases = false)
        assertEquals(listOf("kumi-0.1.0.kumi"), stable.map { it.asset })
        val all = ReleaseAsset.parseReleases(json, includePrereleases = true)
        assertEquals(listOf("kumi-0.2.0.kumi", "kumi-0.1.0.kumi"), all.map { it.asset })
        assertTrue(all[0].prerelease)
        assertEquals("First", all[1].title)
    }

    @Test
    fun theStockSectionHidesOnceKumiIsInstalledUnlessAsked() {
        assertEquals(HomeKind.STOCK, Routing.home(CaseKind.STOCK, kumiSeen = true))
        assertEquals(HomeKind.KUMI, Routing.home(CaseKind.KUMI, kumiSeen = false))
        assertEquals(HomeKind.KUMI, Routing.home(CaseKind.NONE, kumiSeen = true))
        assertEquals(HomeKind.STOCK, Routing.home(CaseKind.SERVICE, kumiSeen = false))
        assertTrue(Routing.legacyVisible(CaseKind.STOCK, kumiSeen = false, showLegacy = false))
        assertFalse(Routing.legacyVisible(CaseKind.KUMI, kumiSeen = true, showLegacy = false))
        assertTrue(Routing.legacyVisible(CaseKind.KUMI, kumiSeen = true, showLegacy = true))
        assertFalse(Routing.legacyVisible(CaseKind.NONE, kumiSeen = true, showLegacy = false))
        val v = KumiVersion(0, 1, 0)
        assertTrue(Routing.offerInstall(CaseKind.STOCK, v, dismissed = null))
        assertFalse(Routing.offerInstall(CaseKind.STOCK, v, dismissed = "0.1.0"))
        assertTrue(Routing.offerInstall(CaseKind.STOCK, KumiVersion(0, 2, 0), dismissed = "0.1.0"))
        assertFalse(Routing.offerInstall(CaseKind.KUMI, v, dismissed = null))
        assertFalse(Routing.offerInstall(CaseKind.STOCK, null, dismissed = null))
    }

    @Test
    fun theNewestVerifiedPackageIsOfferedAsAnUpdateOnlyWhenNewer() {
        fun pkg(v: String, source: FirmwareSource, generation: Long = 1) =
            FirmwarePackage(source, "$v.kumi", PackageInfo(KumiVersion.parse(v)!!, generation, v, 1, 1, "00"), File("$v.kumi"))
        val packages = listOf(pkg("0.1.0", FirmwareSource.BUNDLED), pkg("0.2.0", FirmwareSource.RELEASE), pkg("0.2.0", FirmwareSource.BUNDLED))
        assertEquals(FirmwareSource.RELEASE, FirmwareCatalog.newest(packages)?.source)
        assertEquals(KumiVersion(0, 2, 0), FirmwareCatalog.updateFor(KumiVersion(0, 1, 0), packages)?.version)
        assertNull(FirmwareCatalog.updateFor(KumiVersion(0, 2, 0), packages))
        assertEquals(KumiVersion(0, 2, 0), FirmwareCatalog.updateFor(null, packages)?.version)
    }

    @Test
    fun keyActionsReadNaturally() {
        assertEquals("Ctrl+Shift+C", HidUsages.describe(KeyAction.KeyWithModifiers(0x06, 0x03)))
        assertEquals("Caps Lock", HidUsages.key(0x39))
        assertEquals("Esc", HidUsages.describe(KeyAction.Key(0x29)))
        assertEquals("Play/Pause", HidUsages.describe(KeyAction.Consumer(0xCD)))
        assertEquals("Hold layer 2", HidUsages.describe(KeyAction.LayerHold(1)))
        assertEquals("F12", HidUsages.key(0x45))
    }

    @Test
    fun storedBackupsRoundTrip() {
        val backup = StoredBackup("/data/x.bin", "ab".repeat(32), "Head", 1_700_000_000_000)
        assertEquals(backup, StoredBackup.parse(backup.serialize()))
        assertNull(StoredBackup.parse("{broken"))
    }

    @Test
    fun appReleasesParseSignedApksAndDetectUpdates() {
        val json = """
            [
              {"tag_name":"v1.2.0","name":"Release 1.2","body":"- Better EQ\n- Faster updates","draft":false,"prerelease":false,"published_at":"2026-11-25T00:00:00Z",
               "assets":[{"name":"snap-case-1.2.0.apk","size":12000000,"browser_download_url":"https://github.com/o/kumi/releases/download/v1.2.0/snap-case-1.2.0.apk"},
                         {"name":"snap-case-1.2.0-unsigned.apk","size":12000000,"browser_download_url":"https://github.com/o/kumi/releases/download/v1.2.0/snap-case-1.2.0-unsigned.apk"}]},
              {"tag_name":"v1.2.0-rc1","name":"RC1","body":"rc","draft":false,"prerelease":true,"published_at":"2026-11-24T00:00:00Z",
               "assets":[{"name":"snap-case-1.2.0-rc1.apk","size":11500000,"browser_download_url":"https://github.com/o/kumi/releases/download/v1.2.0-rc1/snap-case-1.2.0-rc1.apk"}]}
            ]
        """.trimIndent()
        val all = AppReleaseAsset.parseReleases(json, includePrereleases = true)
        assertEquals(listOf("snap-case-1.2.0.apk", "snap-case-1.2.0-rc1.apk"), all.map { it.asset })
        assertEquals("snap-case-1.2.0.apk", FirmwareCatalog.appUpdateFor("1.0.0", all)?.asset)
        assertEquals("snap-case-1.2.0.apk", FirmwareCatalog.appUpdateFor("1.2.0-rc1", all)?.asset)
        assertNull(FirmwareCatalog.appUpdateFor("1.2.0", all))
    }

    @Test
    fun combinedReleaseParsesDistinctFirmwareAndAppAssetVersions() {
        assertEquals(KumiVersion(0, 1, 28), KumiVersion.extractFromFilename("kumi-0.1.28.kumi"))
        assertEquals(KumiVersion(0, 2, 1), KumiVersion.extractFromFilename("kumi-app-0.2.1-release.apk"))
        assertNull(KumiVersion.extractFromFilename("kumi-latest.apk"))
        val json = """
            [
              {"tag_name":"v0.1.28","name":"Kumi 0.1.28 + App 0.2.1","body":"Combined release","draft":false,"prerelease":false,"published_at":"2026-11-25T12:00:00Z",
               "assets":[{"name":"kumi-0.1.28.kumi","size":120000,"browser_download_url":"https://github.com/iasnezhkov/kumi/releases/download/v0.1.28/kumi-0.1.28.kumi"},
                         {"name":"kumi-app-0.2.1-release.apk","size":18000000,"browser_download_url":"https://github.com/iasnezhkov/kumi/releases/download/v0.1.28/kumi-app-0.2.1-release.apk"}]}
            ]
        """.trimIndent()
        val fw = ReleaseAsset.parseReleases(json, includePrereleases = false)
        val app = AppReleaseAsset.parseReleases(json, includePrereleases = false)
        assertEquals(listOf(KumiVersion(0, 1, 28)), fw.map { it.version })
        assertEquals(listOf(KumiVersion(0, 2, 1)), app.map { it.version })
        assertEquals("kumi-app-0.2.1-release.apk", FirmwareCatalog.appUpdateFor("0.2.0", app)?.asset)
        assertNull(FirmwareCatalog.appUpdateFor("0.2.1", app))
    }

    @Test
    fun builtInAndCustomEqPresetsAreValidAndRoundTrip() {
        assertEquals(8, KumiEqPresets.STANDARDS.size)
        assertEquals(8, KumiEqPresets.GENRES.size)
        assertEquals(16, KumiEqPresets.ALL.size)
        assertEquals(16, KumiEqPresets.ALL.map { it.id }.toSet().size)
        for (preset in KumiEqPresets.ALL) {
            assertTrue("preset ${preset.id} must be valid", EqProfile(0, preset.bands).valid)
        }
        val saved = listOf(
            CustomEqPreset("My IEM", EqProfile(-450, listOf(ParametricBand(BandKind.LowShelf, 105, 710, 450)))),
            CustomEqPreset("Late Night", EqProfile(-550, KumiEqPresets.ALL.first { it.id == KumiEqPresets.LATE_NIGHT }.bands)),
        )
        val encoded = CustomEqPreset.serializeList(saved)
        assertEquals(saved, CustomEqPreset.parseList(encoded))
    }
}
