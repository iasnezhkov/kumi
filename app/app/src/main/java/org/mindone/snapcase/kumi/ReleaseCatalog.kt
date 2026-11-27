// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import java.io.File
import java.io.InputStream
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

data class CatalogState(
    val packages: List<FirmwarePackage> = emptyList(),
    val releases: List<ReleaseAsset> = emptyList(),
    val appReleases: List<AppReleaseAsset> = emptyList(),
    val downloadedApks: Map<String, File> = emptyMap(),
    val checking: Boolean = false,
    val downloading: String? = null,
    val downloadingApp: String? = null,
    val appDownloadProgress: Float? = null,
    val releaseError: String? = null,
    val noReleases: Boolean = false,
    val nativeMissing: Boolean = false,
    val keyMissing: Boolean = false,
    val demo: Boolean = false,
) {
    val newest: FirmwarePackage? get() = FirmwareCatalog.newest(packages)

    fun appUpdate(currentVersion: String): AppReleaseAsset? = FirmwareCatalog.appUpdateFor(currentVersion, appReleases)
}

interface BundledPackages {
    fun names(): List<String>
    fun open(path: String): InputStream
}

interface DemoPackages {
    val publicKey: String
    fun build(version: KumiVersion, generation: Int): ByteArray
}

data class ReleaseSettings(val check: Boolean, val includePrereleases: Boolean, val repository: String)

class ReleaseCatalog(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val bridge: KumiBridge,
    private val cacheDir: File,
    private val bundled: BundledPackages,
    private val demoPackages: DemoPackages,
    private val releaseKey: String,
    private val defaultRepository: String,
    private val settings: () -> ReleaseSettings,
) {
    private val _state = MutableStateFlow(CatalogState(nativeMissing = !bridge.available))
    val state: StateFlow<CatalogState> = _state.asStateFlow()

    @Volatile private var demo = false
    private val fetchMutex = Mutex()

    val publicKey: String get() = if (demo) demoPackages.publicKey else releaseKey

    private fun catalog() = FirmwareCatalog(bridge, publicKey, cacheDir)

    fun reload(demo: Boolean, caseHasKumi: Boolean): Job {
        this.demo = demo
        return scope.launch(io) {
            if (!bridge.available) {
                _state.value = CatalogState(nativeMissing = true, demo = demo)
                return@launch
            }
            val packages = when {
                demo -> demoPackages(caseHasKumi)
                releaseKey.isBlank() -> emptyList()
                else -> installed()
            }
            val cachedApks = if (demo) emptyMap() else cachedApks()
            _state.update {
                CatalogState(
                    packages = packages,
                    releases = if (demo) emptyList() else it.releases,
                    appReleases = if (demo) emptyList() else it.appReleases,
                    downloadedApks = cachedApks,
                    keyMissing = !demo && releaseKey.isBlank(),
                    demo = demo,
                )
            }
            if (!demo && settings().check) fetchReleases()
        }
    }

    private fun cachedApks(): Map<String, File> {
        val dir = File(cacheDir, FirmwareCatalog.APP_UPDATES_DIR)
        return dir.listFiles { f -> f.isFile && f.name.endsWith(".${AppReleaseAsset.EXTENSION}") }
            ?.associateBy { it.name } ?: emptyMap()
    }

    private fun installed(): List<FirmwarePackage> {
        val catalog = catalog()
        val names = runCatching { bundled.names() }.getOrDefault(emptyList())
        val fromApp = catalog.bundled(names, bundled::open)
        val downloaded = File(cacheDir, "releases").listFiles { f -> f.name.endsWith(".${ReleaseAsset.EXTENSION}") }
            ?.mapNotNull { runCatching { catalog.verify(FirmwareSource.RELEASE, it.name, it, catalog.readNotes(it)) }.getOrNull() } ?: emptyList()
        return fromApp + downloaded
    }

    private fun demoPackages(caseHasKumi: Boolean): List<FirmwarePackage> {
        val catalog = catalog()
        val folder = File(cacheDir, "demo").apply { mkdirs() }
        val all = DEMO_VERSIONS.mapNotNull { (version, generation) ->
            runCatching {
                val file = File(folder, "kumi-demo-$version.${ReleaseAsset.EXTENSION}")
                file.writeBytes(demoPackages.build(version, generation))
                catalog.verify(FirmwareSource.DEMO, file.name, file)
            }.getOrNull()
        }
        return if (caseHasKumi) all else all.take(1)
    }

    fun checkReleases(): Job = scope.launch(io) {
        if (!demo) fetchReleases()
    }

    fun clearReleaseError() {
        _state.update { it.copy(releaseError = null) }
    }

    private suspend fun fetchReleases() = fetchMutex.withLock {
        val config = settings()
        val repository = config.repository.ifBlank { defaultRepository }
        _state.update { it.copy(checking = true, releaseError = null, noReleases = false) }
        val result = runCatching { catalog().releaseFeed(repository, config.includePrereleases) }
        _state.update { state ->
            result.fold(
                onSuccess = { feed ->
                    state.copy(
                        checking = false,
                        releases = feed.firmware,
                        appReleases = feed.app,
                        noReleases = feed.firmware.isEmpty() && feed.app.isEmpty(),
                    )
                },
                onFailure = {
                    if (it is NoReleasesException) state.copy(checking = false, noReleases = true, releases = emptyList(), appReleases = emptyList())
                    else state.copy(checking = false, releaseError = it.message ?: it.toString())
                },
            )
        }
    }

    fun download(asset: ReleaseAsset): Job = scope.launch(io) {
        if (demo || releaseKey.isBlank()) return@launch
        _state.update { it.copy(downloading = asset.asset, releaseError = null) }
        val result = runCatching { catalog().download(asset) }
        _state.update { state ->
            result.fold(
                onSuccess = { pkg -> state.copy(downloading = null, packages = state.packages.filterNot { it.file == pkg.file } + pkg) },
                onFailure = { state.copy(downloading = null, releaseError = it.message ?: it.toString()) },
            )
        }
    }

    fun downloadApp(asset: AppReleaseAsset, onReady: (File) -> Unit = {}): Job = scope.launch(io) {
        if (demo) return@launch
        _state.update { it.copy(downloadingApp = asset.asset, appDownloadProgress = 0f, releaseError = null) }
        var lastPct = -1
        val result = runCatching {
            catalog().downloadApp(asset) { progress ->
                val pct = (progress * 100f).toInt().coerceIn(0, 100)
                if (pct != lastPct) {
                    lastPct = pct
                    _state.update { s -> s.copy(appDownloadProgress = pct / 100f) }
                }
            }
        }
        result.fold(
            onSuccess = { file ->
                _state.update { s ->
                    s.copy(
                        downloadingApp = null,
                        appDownloadProgress = null,
                        downloadedApks = s.downloadedApks + (asset.asset to file),
                    )
                }
                withContext(Dispatchers.Main) { onReady(file) }
            },
            onFailure = { error ->
                _state.update { s ->
                    s.copy(
                        downloadingApp = null,
                        appDownloadProgress = null,
                        releaseError = error.message ?: error.toString(),
                    )
                }
            },
        )
    }

    companion object {
        private val DEMO_VERSIONS = listOf(KumiVersion(0, 1, 0) to 1, KumiVersion(0, 2, 0) to 2)
    }
}
