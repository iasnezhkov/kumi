// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import java.io.File
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import org.json.JSONArray

enum class FirmwareSource { BUNDLED, RELEASE, DEMO }

data class ReleaseVersion(val core: KumiVersion, val prerelease: String? = null) : Comparable<ReleaseVersion> {
    val isPrerelease: Boolean get() = !prerelease.isNullOrBlank()

    override fun toString(): String = if (isPrerelease) "$core-$prerelease" else core.toString()

    override fun compareTo(other: ReleaseVersion): Int {
        val coreCmp = core.compareTo(other.core)
        if (coreCmp != 0) return coreCmp
        val a = prerelease?.takeIf { it.isNotBlank() }
        val b = other.prerelease?.takeIf { it.isNotBlank() }
        if (a == null && b == null) return 0
        if (a == null) return 1
        if (b == null) return -1
        val ta = tokenize(a)
        val tb = tokenize(b)
        val n = minOf(ta.size, tb.size)
        for (i in 0 until n) {
            val sa = ta[i]
            val sb = tb[i]
            val ia = sa.toIntOrNull()
            val ib = sb.toIntOrNull()
            val cmp = when {
                ia != null && ib != null -> ia.compareTo(ib)
                ia != null -> -1
                ib != null -> 1
                else -> sa.lowercase().compareTo(sb.lowercase())
            }
            if (cmp != 0) return cmp
        }
        return ta.size.compareTo(tb.size)
    }

    companion object {
        private val FULL = Regex("""^v?(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?:-([0-9A-Za-z._-]+))?(?:\+.*)?$""")
        private val SHA_TAIL = Regex("""-[0-9a-fA-F]{7,40}$""")
        private val PRE_FROM_NAME = Regex("""-(\d{1,3}\.\d{1,3}\.\d{1,3})-((?:rc|alpha|beta|dev|pre)[0-9A-Za-z._-]*)""", RegexOption.IGNORE_CASE)
        private val TOKEN = Regex("""[0-9]+|[A-Za-z]+""")

        private fun tokenize(label: String): List<String> = TOKEN.findAll(label).map { it.value }.toList()

        fun parse(text: String): ReleaseVersion? {
            val match = FULL.matchEntire(text.trim()) ?: return null
            val core = KumiVersion(match.groupValues[1].toInt(), match.groupValues[2].toInt(), match.groupValues[3].toInt())
            val pre = match.groupValues[4].takeIf { it.isNotBlank() && !it.equals("release", ignoreCase = true) }
            return ReleaseVersion(core, pre)
        }

        fun fromBuild(version: KumiVersion, build: String?, filename: String? = null): ReleaseVersion {
            val cleanBuild = build?.trim()?.replace(SHA_TAIL, "")
            val parsedBuild = cleanBuild?.let { parse(it) }
            if (parsedBuild != null && parsedBuild.core == version) return parsedBuild
            val fromName = filename?.substringBeforeLast('.')?.let { base ->
                PRE_FROM_NAME.find(base)?.let { m ->
                    val parsedCore = KumiVersion.parse(m.groupValues[1])
                    if (parsedCore == version) ReleaseVersion(version, m.groupValues[2]) else null
                }
            }
            return fromName ?: ReleaseVersion(version, null)
        }

        fun fromRelease(core: KumiVersion?, tag: String, asset: String, prerelease: Boolean): ReleaseVersion? {
            val v = core ?: return null
            if (!prerelease) return ReleaseVersion(v, null)
            val fromTag = tag.trim().removePrefix("app-").substringAfter('-', "").takeIf { it.isNotBlank() && tag.contains('-') }
            val fromAsset = asset.substringBeforeLast('.').let { base ->
                PRE_FROM_NAME.find(base)?.groupValues?.getOrNull(2)
            }
            return ReleaseVersion(v, fromTag ?: fromAsset ?: "pre")
        }
    }
}

data class FirmwarePackage(
    val source: FirmwareSource,
    val name: String,
    val info: PackageInfo,
    val file: File,
    val notes: String? = null,
) {
    val version: KumiVersion get() = info.version
    val releaseVersion: ReleaseVersion get() = ReleaseVersion.fromBuild(info.version, info.build, name)
    val displayVersion: String get() = releaseVersion.toString()
    fun bytes(): ByteArray = file.readBytes()
}

data class ReleaseAsset(
    val tag: String,
    val title: String,
    val notes: String,
    val prerelease: Boolean,
    val published: String,
    val asset: String,
    val url: String,
    val size: Long,
) {
    val version: KumiVersion? get() = KumiVersion.extractFromFilename(asset) ?: KumiVersion.parse(tag)
    val releaseVersion: ReleaseVersion? get() = ReleaseVersion.fromRelease(version, tag, asset, prerelease)
    val displayVersion: String get() = releaseVersion?.toString() ?: tag.removePrefix("v")

    companion object {
        const val EXTENSION = "kumi"
        const val MAX_SIZE = 2L * 1024 * 1024

        fun parseReleases(json: String, includePrereleases: Boolean): List<ReleaseAsset> {
            val releases = JSONArray(json)
            val out = mutableListOf<ReleaseAsset>()
            for (release in releases.objects()) {
                if (release.optBoolean("draft")) continue
                val prerelease = release.optBoolean("prerelease")
                if (prerelease && !includePrereleases) continue
                val assets = release.optJSONArray("assets") ?: continue
                for (asset in assets.objects()) {
                    val name = asset.optString("name")
                    val size = asset.optLong("size")
                    if (!name.endsWith(".$EXTENSION") || size <= 0 || size > MAX_SIZE) continue
                    out += ReleaseAsset(
                        tag = release.optString("tag_name"),
                        title = release.optString("name").ifBlank { release.optString("tag_name") },
                        notes = release.optString("body"),
                        prerelease = prerelease,
                        published = release.optString("published_at"),
                        asset = name,
                        url = asset.optString("browser_download_url"),
                        size = size,
                    )
                }
            }
            return out.filter { ReleaseHosts.download(it.url) }
                .sortedWith(compareByDescending<ReleaseAsset> { it.releaseVersion }.thenByDescending { it.published })
        }
    }
}

data class AppReleaseAsset(
    val tag: String,
    val title: String,
    val notes: String,
    val prerelease: Boolean,
    val published: String,
    val asset: String,
    val url: String,
    val size: Long,
) {
    val version: KumiVersion?
        get() = KumiVersion.extractFromFilename(asset)
            ?: KumiVersion.parse(tag)
            ?: KumiVersion.parse(tag.removePrefix("app-"))
    val releaseVersion: ReleaseVersion? get() = ReleaseVersion.fromRelease(version, tag, asset, prerelease)
    val displayVersion: String get() = releaseVersion?.toString() ?: tag.removePrefix("v")

    companion object {
        const val EXTENSION = "apk"
        const val MAX_SIZE = 128L * 1024 * 1024

        fun parseReleases(json: String, includePrereleases: Boolean): List<AppReleaseAsset> {
            val releases = JSONArray(json)
            val out = mutableListOf<AppReleaseAsset>()
            for (release in releases.objects()) {
                if (release.optBoolean("draft")) continue
                val prerelease = release.optBoolean("prerelease")
                if (prerelease && !includePrereleases) continue
                val assets = release.optJSONArray("assets") ?: continue
                for (asset in assets.objects()) {
                    val name = asset.optString("name")
                    val size = asset.optLong("size")
                    if (!name.endsWith(".$EXTENSION") || name.contains("unsigned", ignoreCase = true) || size <= 0 || size > MAX_SIZE) continue
                    out += AppReleaseAsset(
                        tag = release.optString("tag_name"),
                        title = release.optString("name").ifBlank { release.optString("tag_name") },
                        notes = release.optString("body"),
                        prerelease = prerelease,
                        published = release.optString("published_at"),
                        asset = name,
                        url = asset.optString("browser_download_url"),
                        size = size,
                    )
                }
            }
            return out.filter { ReleaseHosts.download(it.url) }
                .sortedWith(compareByDescending<AppReleaseAsset> { it.releaseVersion }.thenByDescending { it.published })
        }
    }
}

data class ReleaseFeed(
    val firmware: List<ReleaseAsset>,
    val app: List<AppReleaseAsset>,
)

class FirmwareCatalog(
    private val bridge: KumiBridge,
    private val publicKey: String,
    private val cacheDir: File,
) {
    fun verify(source: FirmwareSource, name: String, file: File, notes: String? = null): FirmwarePackage {
        val info = PackageInfo.from(Replies.parse(bridge.openPackage(file.readBytes(), publicKey)).value as org.json.JSONObject)
        return FirmwarePackage(source, name, info, file, notes)
    }

    fun bundled(names: List<String>, open: (String) -> java.io.InputStream): List<FirmwarePackage> {
        val dir = File(cacheDir, "bundled").apply { mkdirs() }
        return names.filter { it.endsWith(".${ReleaseAsset.EXTENSION}") }.mapNotNull { name ->
            runCatching {
                val target = File(dir, name)
                open("$ASSET_DIR/$name").use { input -> target.outputStream().use { input.copyTo(it) } }
                verify(FirmwareSource.BUNDLED, name, target)
            }.getOrNull()
        }
    }

    fun releases(repository: String, includePrereleases: Boolean): List<ReleaseAsset> =
        releaseFeed(repository, includePrereleases).firmware

    fun releaseFeed(repository: String, includePrereleases: Boolean): ReleaseFeed {
        require(isValidRepository(repository)) { "Repository must look like owner/name" }
        val etagFile = File(cacheDir, "releases-etag.txt")
        val cacheFile = File(cacheDir, "releases-cache.json")
        val key = "$repository|$includePrereleases"
        val saved = etagFile.takeIf { it.isFile }?.readText()?.lines()
        val cachedEtag = if (saved != null && saved.size >= 2 && saved[0] == key && cacheFile.isFile) saved[1] else null
        val text = fetchJson(URL("https://${ReleaseHosts.API}/repos/$repository/releases?per_page=20"), cachedEtag, etagFile, cacheFile, key)
        return ReleaseFeed(
            firmware = ReleaseAsset.parseReleases(text, includePrereleases),
            app = AppReleaseAsset.parseReleases(text, includePrereleases),
        )
    }

    fun download(asset: ReleaseAsset): FirmwarePackage {
        val dir = File(cacheDir, "releases").apply { mkdirs() }
        val safe = asset.asset.replace(Regex("[^A-Za-z0-9._-]"), "_")
        val target = File(dir, safe)
        val partial = File(dir, "$safe.part")
        partial.writeBytes(fetch(URL(asset.url), ReleaseAsset.MAX_SIZE.toInt(), json = false))
        try {
            verify(FirmwareSource.RELEASE, asset.asset, partial, asset.notes)
        } catch (error: Exception) {
            partial.delete()
            throw error
        }
        if (target.exists()) target.delete()
        if (!partial.renameTo(target)) throw IllegalStateException("Cannot store ${asset.asset}")
        runCatching {
            val notesFile = notesSidecar(target)
            if (asset.notes.isNotBlank()) notesFile.writeText(asset.notes) else notesFile.delete()
        }
        return verify(FirmwareSource.RELEASE, asset.asset, target, asset.notes)
    }

    fun readNotes(file: File): String? =
        notesSidecar(file).takeIf { it.isFile }?.runCatching { readText() }?.getOrNull()?.takeIf { it.isNotBlank() }

    private fun notesSidecar(file: File): File = File(file.parentFile, "${file.name}.notes.txt")

    fun downloadApp(asset: AppReleaseAsset, onProgress: (Float) -> Unit): File {
        val dir = File(cacheDir, APP_UPDATES_DIR).apply { mkdirs() }
        val safe = asset.asset.replace(Regex("[^A-Za-z0-9._-]"), "_")
        val target = File(dir, safe)
        val partial = File(dir, "$safe.part")
        try {
            fetchToFile(URL(asset.url), AppReleaseAsset.MAX_SIZE, asset.size, partial, onProgress)
            val header = ByteArray(4)
            partial.inputStream().use { if (it.read(header) != 4) throw IllegalStateException("Downloaded APK is truncated") }
            if (header[0] != 0x50.toByte() || header[1] != 0x4B.toByte() || header[2] != 0x03.toByte() || header[3] != 0x04.toByte()) {
                throw IllegalStateException("Downloaded file is not a valid APK")
            }
            if (target.exists()) target.delete()
            if (!partial.renameTo(target)) throw IllegalStateException("Cannot store ${asset.asset}")
            return target
        } catch (error: Exception) {
            partial.delete()
            throw error
        }
    }

    private fun fetchJson(url: URL, etag: String?, etagFile: File, cacheFile: File, key: String): String {
        val out = java.io.ByteArrayOutputStream()
        var responseEtag: String? = null
        var notModified = false
        openStream(url, json = true, ifNoneMatch = etag, onEtag = { responseEtag = it }, onNotModified = { notModified = true }) { input, _ ->
            val buffer = ByteArray(16 * 1024)
            while (true) {
                val n = input.read(buffer)
                if (n < 0) break
                out.write(buffer, 0, n)
                if (out.size() > JSON_LIMIT) throw IllegalStateException("Response larger than $JSON_LIMIT bytes")
            }
        }
        if (notModified && cacheFile.isFile) {
            return cacheFile.readText(Charsets.UTF_8)
        }
        val text = out.toString(Charsets.UTF_8)
        if (!responseEtag.isNullOrBlank()) {
            runCatching {
                cacheDir.mkdirs()
                cacheFile.writeText(text, Charsets.UTF_8)
                etagFile.writeText("$key\n$responseEtag")
            }
        }
        return text
    }

    private fun fetch(url: URL, limit: Int, json: Boolean): ByteArray {
        val out = java.io.ByteArrayOutputStream()
        openStream(url, json) { input, _ ->
            val buffer = ByteArray(16 * 1024)
            while (true) {
                val n = input.read(buffer)
                if (n < 0) break
                out.write(buffer, 0, n)
                if (out.size() > limit) throw IllegalStateException("Response larger than $limit bytes")
            }
        }
        return out.toByteArray()
    }

    private fun fetchToFile(url: URL, limit: Long, expectedSize: Long, target: File, onProgress: (Float) -> Unit) {
        openStream(url, json = false) { input, contentLength ->
            val total = if (contentLength > 0L) contentLength else expectedSize
            var written = 0L
            FileOutputStream(target).use { out ->
                val buffer = ByteArray(32 * 1024)
                while (true) {
                    val n = input.read(buffer)
                    if (n < 0) break
                    written += n
                    if (written > limit) throw IllegalStateException("Response larger than $limit bytes")
                    out.write(buffer, 0, n)
                    if (total > 0L) onProgress((written.toFloat() / total.toFloat()).coerceIn(0f, 1f))
                }
            }
        }
    }

    private fun openStream(
        url: URL,
        json: Boolean,
        ifNoneMatch: String? = null,
        onEtag: (String?) -> Unit = {},
        onNotModified: () -> Unit = {},
        consume: (java.io.InputStream, Long) -> Unit,
    ) {
        var current = url
        repeat(MAX_REDIRECTS) {
            if (!ReleaseHosts.allowed(current, json)) throw IllegalStateException("Refusing to contact ${current.protocol}://${current.host}")
            val connection = current.openConnection() as HttpURLConnection
            connection.instanceFollowRedirects = false
            connection.connectTimeout = TIMEOUT_MS
            connection.readTimeout = TIMEOUT_MS
            connection.setRequestProperty("User-Agent", "kumi-app")
            if (json) {
                connection.setRequestProperty("Accept", "application/vnd.github+json")
                if (!ifNoneMatch.isNullOrBlank()) connection.setRequestProperty("If-None-Match", ifNoneMatch)
            }
            try {
                when (val code = connection.responseCode) {
                    304 -> {
                        onNotModified()
                        return
                    }
                    in 300..399 -> {
                        current = URL(current, connection.getHeaderField("Location") ?: throw IllegalStateException("Redirect without location"))
                    }
                    200 -> {
                        onEtag(connection.getHeaderField("ETag"))
                        connection.inputStream.use { input -> consume(input, connection.contentLengthLong) }
                        return
                    }
                    404 -> throw NoReleasesException()
                    else -> throw IllegalStateException("HTTP $code from ${current.host}")
                }
            } finally {
                connection.disconnect()
            }
        }
        throw IllegalStateException("Too many redirects")
    }

    companion object {
        const val ASSET_DIR = "kumi"
        const val APP_UPDATES_DIR = "app-updates"
        private const val TIMEOUT_MS = 15_000
        private const val JSON_LIMIT = 1024 * 1024
        private const val MAX_REDIRECTS = 5
        private val REPOSITORY = Regex("""^[A-Za-z0-9-]{1,39}/[A-Za-z0-9._-]{1,100}$""")

        fun isValidRepository(repository: String): Boolean = REPOSITORY.matches(repository)

        fun newest(packages: List<FirmwarePackage>): FirmwarePackage? =
            packages.maxWithOrNull(
                compareBy<FirmwarePackage> { it.releaseVersion }
                    .thenBy { it.info.generation }
                    .thenBy { it.source != FirmwareSource.BUNDLED },
            )

        fun isPackageNewerThan(pkg: FirmwarePackage, installed: KumiVersion?, installedBuild: String? = null): Boolean {
            if (installed == null) return true
            if (pkg.version != installed) return pkg.version > installed
            if (installedBuild.isNullOrBlank()) return false
            return pkg.releaseVersion > ReleaseVersion.fromBuild(installed, installedBuild)
        }

        fun isReleaseNewerThan(asset: ReleaseAsset, installed: KumiVersion?, installedBuild: String? = null): Boolean {
            val candidate = asset.releaseVersion ?: return false
            if (installed == null) return true
            return candidate > ReleaseVersion.fromBuild(installed, installedBuild)
        }

        fun updateFor(installed: KumiVersion?, packages: List<FirmwarePackage>, installedBuild: String? = null): FirmwarePackage? =
            newest(packages)?.takeIf { isPackageNewerThan(it, installed, installedBuild) }

        fun releaseUpdateFor(installed: KumiVersion?, releases: List<ReleaseAsset>, installedBuild: String? = null): ReleaseAsset? =
            releases.firstOrNull { asset -> isReleaseNewerThan(asset, installed, installedBuild) }

        fun latestFirmwareUpdate(
            installed: KumiVersion?,
            packages: List<FirmwarePackage>,
            releases: List<ReleaseAsset>,
            installedBuild: String? = null,
        ): ReleaseVersion? = listOfNotNull(
            updateFor(installed, packages, installedBuild)?.releaseVersion,
            releaseUpdateFor(installed, releases, installedBuild)?.releaseVersion,
        ).maxOrNull()

        fun latestFirmwareUpdateVersion(
            installed: KumiVersion?,
            packages: List<FirmwarePackage>,
            releases: List<ReleaseAsset>,
            installedBuild: String? = null,
        ): KumiVersion? = latestFirmwareUpdate(installed, packages, releases, installedBuild)?.core

        fun appUpdateFor(currentVersion: String, releases: List<AppReleaseAsset>): AppReleaseAsset? {
            val installed = ReleaseVersion.parse(currentVersion) ?: return null
            return releases.firstOrNull { asset ->
                val v = asset.releaseVersion ?: return@firstOrNull false
                v > installed
            }
        }
    }
}

class NoReleasesException : Exception("No releases published yet")

object ReleaseHosts {
    const val API = "api.github.com"
    private val DOWNLOADS = setOf("github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com")

    fun allowed(url: URL, json: Boolean): Boolean =
        url.protocol == "https" && (url.port == -1 || url.port == 443) && url.userInfo == null &&
            url.host.lowercase() in (if (json) setOf(API) else DOWNLOADS)

    fun download(url: String): Boolean = runCatching { allowed(URL(url), json = false) }.getOrDefault(false)
}
