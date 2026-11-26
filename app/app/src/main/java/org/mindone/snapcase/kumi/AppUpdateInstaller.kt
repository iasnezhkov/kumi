// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.Signature
import android.net.Uri
import android.provider.Settings
import androidx.core.content.FileProvider
import java.io.File
import org.mindone.snapcase.BuildConfig

sealed interface AppInstallResult {
    data object Launched : AppInstallResult
    data object PermissionRequested : AppInstallResult
    data object InvalidApk : AppInstallResult
    data object LaunchFailed : AppInstallResult
    data class PackageMismatch(val actual: String) : AppInstallResult
    data class SignatureMismatch(val systemBuild: Boolean) : AppInstallResult
}

object AppUpdateInstaller {
    fun install(context: Context, apkFile: File): AppInstallResult {
        if (!apkFile.isFile) return AppInstallResult.InvalidApk
        val pm = context.packageManager
        val flags = PackageManager.GET_SIGNING_CERTIFICATES
        val archive = runCatching {
            pm.getPackageArchiveInfo(apkFile.path, PackageManager.PackageInfoFlags.of(flags.toLong()))
        }.getOrNull() ?: return AppInstallResult.InvalidApk
        if (archive.packageName != context.packageName) {
            return AppInstallResult.PackageMismatch(archive.packageName)
        }
        val installed = runCatching {
            pm.getPackageInfo(context.packageName, PackageManager.PackageInfoFlags.of(flags.toLong()))
        }.getOrNull()
        val archiveSigners = archive.signingInfo?.apkContentsSigners
        val installedSigners = installed?.signingInfo?.apkContentsSigners
        if (archiveSigners != null && installedSigners != null && !signaturesMatch(archiveSigners, installedSigners)) {
            return AppInstallResult.SignatureMismatch(BuildConfig.SYSTEM_BUILD)
        }
        if (!pm.canRequestPackageInstalls()) {
            val perm = Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES).apply {
                data = Uri.parse("package:${context.packageName}")
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            return if (runCatching { context.startActivity(perm) }.isSuccess) {
                AppInstallResult.PermissionRequested
            } else {
                AppInstallResult.LaunchFailed
            }
        }
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.updates", apkFile)
        val intent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "application/vnd.android.package-archive")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        return if (runCatching { context.startActivity(intent) }.isSuccess) {
            AppInstallResult.Launched
        } else {
            AppInstallResult.LaunchFailed
        }
    }

    private fun signaturesMatch(a: Array<Signature>, b: Array<Signature>): Boolean {
        if (a.size != b.size) return false
        val setA = a.map { it.toCharsString() }.toSet()
        val setB = b.map { it.toCharsString() }.toSet()
        return setA == setB
    }
}
