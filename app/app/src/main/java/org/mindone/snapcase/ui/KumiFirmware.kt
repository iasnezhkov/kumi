// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.view.KeyEvent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import java.io.File
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.flow.StateFlow
import org.mindone.snapcase.BuildConfig
import org.mindone.snapcase.R
import org.mindone.snapcase.data.AppPreferences
import org.mindone.snapcase.kumi.AppInstallResult
import org.mindone.snapcase.kumi.AppReleaseAsset
import org.mindone.snapcase.kumi.AppUpdateInstaller
import org.mindone.snapcase.kumi.CatalogState
import org.mindone.snapcase.kumi.FirmwareCatalog
import org.mindone.snapcase.kumi.FirmwarePackage
import org.mindone.snapcase.kumi.FirmwareSource
import org.mindone.snapcase.kumi.FlowKind
import org.mindone.snapcase.kumi.FlowNotice
import org.mindone.snapcase.kumi.FlowState
import org.mindone.snapcase.kumi.FlowStep
import org.mindone.snapcase.kumi.InstallFlow
import org.mindone.snapcase.kumi.KumiSession
import org.mindone.snapcase.kumi.KumiVersion
import org.mindone.snapcase.kumi.ReleaseAsset
import org.mindone.snapcase.kumi.ReleaseCatalog
import org.mindone.snapcase.kumi.ReleaseVersion
import org.mindone.snapcase.kumi.SlotState
import org.mindone.snapcase.kumi.StoredBackup
import org.mindone.snapcase.transport.CaseKind

@Composable
fun InstallOfferCard(pkg: FirmwarePackage, onOpen: () -> Unit, onDismiss: () -> Unit) {
    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.tertiaryContainer,
            contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
        ),
    ) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(stringResource(R.string.kumi_offer_title), style = MaterialTheme.typography.titleMedium)
            Text(stringResource(R.string.kumi_offer_text, pkg.version.toString()), style = MaterialTheme.typography.bodySmall)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = onOpen) { Text(stringResource(R.string.kumi_offer_action)) }
                TextButton(
                    onClick = onDismiss,
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.onTertiaryContainer),
                ) { Text(stringResource(R.string.kumi_offer_later)) }
            }
        }
    }
}

private data class ChangelogPreview(
    val title: String,
    val subtitle: String?,
    val notes: String,
    val releaseUrl: String? = null,
    val primaryLabel: String? = null,
    val onPrimary: (() -> Unit)? = null,
)

@Composable
fun KumiFirmwareScreen(
    kumi: KumiSession,
    catalog: ReleaseCatalog,
    flows: InstallFlow,
    prefs: AppPreferences,
    installBlocked: StateFlow<Boolean>,
    initialPage: Int = 0,
    onBack: () -> Unit = {},
    onOpenDetails: () -> Unit = {},
    onOpenHome: () -> Unit = {},
) {
    val context = LocalContext.current
    val resources = LocalResources.current
    val view = LocalView.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val kind by kumi.kind.collectAsStateWithLifecycle()
    val state by kumi.state.collectAsStateWithLifecycle()
    val firmware by catalog.state.collectAsStateWithLifecycle()
    val flowState by flows.state.collectAsStateWithLifecycle()
    val busy = flowState?.busy == true
    DisposableEffect(view, busy) {
        view.keepScreenOn = busy
        onDispose { view.keepScreenOn = false }
    }
    val blocked by installBlocked.collectAsStateWithLifecycle()
    val settings by prefs.settings.collectAsStateWithLifecycle()
    val installed = state.device?.firmware
    val installedBuild = state.device?.build
    val devChannel = settings.includePrereleases
    val appUpdate = remember(firmware.appReleases) { firmware.appUpdate(BuildConfig.VERSION_NAME) }
    val fwUpdate = remember(installed, installedBuild, firmware.packages, firmware.releases, state.ramOnly) {
        if (installed != null && !state.ramOnly) {
            FirmwareCatalog.latestFirmwareUpdate(installed, firmware.packages, firmware.releases, installedBuild)
        } else {
            null
        }
    }
    val hasUpdateBadge = appUpdate != null || fwUpdate != null
    var confirmInstall by remember { mutableStateOf<FirmwarePackage?>(null) }
    var confirmRestore by remember { mutableStateOf(false) }
    var changelog by remember { mutableStateOf<ChangelogPreview?>(null) }
    var installError by remember { mutableStateOf<String?>(null) }
    var installNotice by remember { mutableStateOf<String?>(null) }
    var pendingApkInstall by remember { mutableStateOf<File?>(null) }
    val backup = remember(settings.stockBackup) { StoredBackup.parse(settings.stockBackup) }
    val backupDate = remember(backup?.takenAt) { backup?.let { DateFormat.getDateTimeInstance().format(Date(it.takenAt)) } }
    val flow = flowState
    val runningSlot = state.slots?.running
    val stockOnboarding = kind == CaseKind.STOCK || kind == CaseKind.SERVICE || (kind == CaseKind.KUMI && state.ramOnly)

    val launchApkInstall: (File) -> Unit = { file ->
        when (val res = AppUpdateInstaller.install(context, file)) {
            AppInstallResult.Launched -> {
                pendingApkInstall = null
                installError = null
                installNotice = null
            }
            AppInstallResult.PermissionRequested -> {
                pendingApkInstall = file
                installError = null
                installNotice = resources.getString(R.string.kumi_updates_err_permission)
            }
            AppInstallResult.InvalidApk -> {
                pendingApkInstall = null
                installNotice = null
                installError = resources.getString(R.string.kumi_updates_err_invalid_apk)
            }
            AppInstallResult.LaunchFailed -> {
                pendingApkInstall = null
                installNotice = null
                installError = resources.getString(R.string.kumi_updates_err_launch)
            }
            is AppInstallResult.PackageMismatch -> {
                pendingApkInstall = null
                installNotice = null
                installError = resources.getString(R.string.kumi_updates_err_package, res.actual)
            }
            is AppInstallResult.SignatureMismatch -> {
                pendingApkInstall = null
                installNotice = null
                installError = resources.getString(
                    if (res.systemBuild) R.string.kumi_updates_err_signature_system else R.string.kumi_updates_err_signature,
                )
            }
        }
    }

    DisposableEffect(lifecycleOwner, context) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                val pending = pendingApkInstall
                if (pending != null && context.packageManager.canRequestPackageInstalls()) {
                    pendingApkInstall = null
                    installNotice = null
                    launchApkInstall(pending)
                }
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    val listState = rememberLazyListState()
    var page by rememberSaveable(initialPage) { mutableIntStateOf(initialPage.coerceIn(0, 2)) }
    LaunchedEffect(flow?.kind, flow?.step) {
        if (flow != null) listState.animateScrollToItem(0)
    }

    val shTab = stringResource(R.string.kumi_key_hint_tab)
    val shBack = stringResource(R.string.action_back)
    val shDetails = stringResource(R.string.kumi_details_title)
    val page0Keys = rememberMnemonicKeys(
        specs = listOf(
            ShortcutSpec("details", shDetails, KeyEvent.KEYCODE_H),
        ),
    )
    if (!stockOnboarding && !busy) {
        RegisterScreenTabs(count = 3, current = page, onSelect = { page = it })
        RegisterScreenScroll(listState)
    }
    RegisterScreenKeys(
        key = listOf(page, stockOnboarding, busy, kind, page0Keys),
        shortcuts = when {
            busy -> emptyList()
            stockOnboarding -> listOf("⌫" to shBack)
            page == 0 && kind == CaseKind.KUMI -> page0Keys.pairs() + listOf("↵" to shTab, "⌫" to shBack)
            else -> listOf("↵" to shTab, "⌫" to shBack)
        },
    ) { code, repeat ->
        if (repeat || busy) return@RegisterScreenKeys false
        if (!stockOnboarding && page == 0 && kind == CaseKind.KUMI && page0Keys.idFor(code) == "details") {
            onOpenDetails()
            true
        } else {
            false
        }
    }

    if (flow != null && busy) {
        Column(
            Modifier
                .fillMaxSize()
                .statusBarsPadding()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 10.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.Center,
        ) {
            FlowCard(
                flow = flow,
                onResume = flows::resume,
                onClose = flows::close,
                onOpenHome = {
                    flows.close()
                    onOpenHome()
                },
            )
        }
        return
    }

    if (stockOnboarding) {
        val latestPkg = firmware.newest
        Column(Modifier.fillMaxSize()) {
            InlineTitleHeader(
                title = stringResource(R.string.kumi_upgrade_screen_title),
                onBack = onBack,
                trailing = {
                    if (!settings.demoMode) {
                        TextButton(
                            onClick = { catalog.checkReleases() },
                            enabled = !firmware.checking,
                            contentPadding = PaddingValues(horizontal = 8.dp, vertical = 0.dp),
                        ) { Text(stringResource(R.string.kumi_updates_check), style = MaterialTheme.typography.labelMedium) }
                    }
                },
            )
            LazyColumn(
                Modifier.weight(1f).fillMaxWidth().padding(horizontal = 10.dp),
                state = listState,
                contentPadding = PaddingValues(bottom = 12.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                if (state.error != null) {
                    item { KumiErrorBanner(state, kumi::clearError) }
                }
                if (flow != null) {
                    item {
                        FlowCard(
                            flow = flow,
                            onResume = flows::resume,
                            onClose = flows::close,
                            onOpenHome = {
                                flows.close()
                                onOpenHome()
                            },
                        )
                    }
                }
                if (kind == CaseKind.SERVICE && flow == null) {
                    item {
                        SectionCard(stringResource(R.string.case_service)) {
                            Note(stringResource(R.string.kumi_service_stuck))
                            Button(onClick = flows::returnToNormal) { Text(stringResource(R.string.action_return_normal)) }
                        }
                    }
                }
                if (appUpdate != null && initialPage == 1) {
                    item {
                        UpdatesCenterCard(
                            firmware = firmware,
                            catalog = catalog,
                            kind = kind,
                            installed = installed,
                            installedBuild = installedBuild,
                            devChannel = devChannel,
                            ramOnly = state.ramOnly,
                            busy = false,
                            blocked = blocked,
                            demo = settings.demoMode,
                            repository = settings.releaseRepository,
                            installError = installError,
                            installNotice = installNotice,
                            onDismissInstallError = { installError = null },
                            onInstallPackage = { confirmInstall = it },
                            onUpdatePackage = { flows.update(it) },
                            onInstallApk = launchApkInstall,
                            onShowChangelog = { changelog = it },
                        )
                    }
                }
                item {
                    Card(
                        Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(16.dp),
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.primaryContainer,
                            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                        ),
                    ) {
                        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            if (firmware.checking) LinearProgressIndicator(Modifier.fillMaxWidth())
                            if (firmware.nativeMissing) Note(stringResource(R.string.kumi_native_missing), MaterialTheme.colorScheme.error)
                            if (firmware.keyMissing) Note(stringResource(R.string.kumi_key_missing), MaterialTheme.colorScheme.error)
                            if (latestPkg != null) {
                                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                                    Text(
                                        stringResource(R.string.kumi_name_version, latestPkg.version.toString()),
                                        style = MaterialTheme.typography.titleMedium,
                                        modifier = Modifier.weight(1f),
                                    )
                                    EvidenceLabel(stringResource(sourceName(latestPkg.source)))
                                }
                                Text(
                                    listOf(
                                        stringResource(R.string.kumi_package_size, (latestPkg.info.stage0 + latestPkg.info.image) / 1024),
                                        stringResource(R.string.kumi_verified),
                                        stringResource(R.string.kumi_why_safe_title),
                                    ).joinToString(" · "),
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.85f),
                                )
                                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                                    if (blocked) {
                                        Note(stringResource(R.string.kumi_install_not_enabled), MaterialTheme.colorScheme.error)
                                    } else {
                                        Button(onClick = { confirmInstall = latestPkg }) {
                                            Text(stringResource(R.string.kumi_install_action))
                                        }
                                    }
                                    latestPkg.notes?.takeIf { it.isNotBlank() }?.let { notes ->
                                        TextButton(
                                            onClick = {
                                                changelog = ChangelogPreview(
                                                    title = "Kumi ${latestPkg.version}",
                                                    subtitle = latestPkg.info.build,
                                                    notes = notes,
                                                )
                                            },
                                            colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.onPrimaryContainer),
                                        ) { Text(stringResource(R.string.kumi_updates_changelog)) }
                                    }
                                }
                            } else {
                                Note(stringResource(R.string.kumi_no_packages))
                            }
                        }
                    }
                }
                item { CompactUpgradeBenefitsCard() }
                if (!(appUpdate != null && initialPage == 1)) {
                    item {
                        UpdatesCenterCard(
                            firmware = firmware,
                            catalog = catalog,
                            kind = kind,
                            installed = installed,
                            installedBuild = installedBuild,
                            devChannel = devChannel,
                            ramOnly = state.ramOnly,
                            busy = false,
                            blocked = blocked,
                            demo = settings.demoMode,
                            repository = settings.releaseRepository,
                            installError = installError,
                            installNotice = installNotice,
                            onDismissInstallError = { installError = null },
                            onInstallPackage = { confirmInstall = it },
                            onUpdatePackage = { flows.update(it) },
                            onInstallApk = launchApkInstall,
                            onShowChangelog = { changelog = it },
                        )
                    }
                }
                if (kind == CaseKind.KUMI && state.ramOnly) {
                    item {
                        SectionCard(stringResource(R.string.kumi_stock_title)) {
                            Note(stringResource(R.string.kumi_ram_restart_text))
                            OutlinedButton(onClick = { kumi.restartCase() }) { Text(stringResource(R.string.kumi_ram_restart)) }
                        }
                    }
                } else if (kind == CaseKind.SERVICE && backup != null) {
                    item {
                        SectionCard(stringResource(R.string.kumi_stock_title)) {
                            if (backupDate != null) InfoRow(stringResource(R.string.kumi_backup_taken), backupDate)
                            OutlinedButton(onClick = { confirmRestore = true }) { Text(stringResource(R.string.kumi_restore_action)) }
                        }
                    }
                }
            }
        }
    } else {
        Column(Modifier.fillMaxSize()) {
            val updatesTabLabel = stringResource(R.string.kumi_case_tab_updates).let { label ->
                if (hasUpdateBadge && page != 1) "$label •" else label
            }
            InlineTabHeader(
                tabs = listOf(
                    stringResource(R.string.kumi_case_tab_status),
                    updatesTabLabel,
                    stringResource(R.string.kumi_case_tab_stock),
                ),
                selectedIndex = page,
                onSelect = { page = it },
                onBack = onBack,
            )
            LazyColumn(
                Modifier.weight(1f).fillMaxWidth().padding(horizontal = 12.dp),
                state = listState,
                contentPadding = PaddingValues(top = 2.dp, bottom = 8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                if (state.error != null) {
                    item { KumiErrorBanner(state, kumi::clearError) }
                }
                if (flow != null) {
                    item {
                        FlowCard(
                            flow = flow,
                            onResume = flows::resume,
                            onClose = flows::close,
                            onOpenHome = {
                                flows.close()
                                onOpenHome()
                            },
                        )
                    }
                }
                when (page) {
                    0 -> {
                        item {
                            SectionCard(stringResource(R.string.kumi_on_case)) {
                                InfoRow(
                                    stringResource(R.string.kumi_case_firmware),
                                    when (kind) {
                                        CaseKind.KUMI -> installed?.let { stringResource(R.string.kumi_name_version, it.toString()) } ?: stringResource(R.string.kumi_name)
                                        else -> stringResource(R.string.kumi_case_absent)
                                    },
                                )
                                if (state.connected && runningSlot?.state == SlotState.Trial) {
                                    Note(stringResource(R.string.kumi_trial_hint))
                                    FilledTonalButton(onClick = { kumi.confirmBoot() }) {
                                        Text(stringResource(R.string.kumi_slot_confirm))
                                    }
                                }
                                if (settings.demoMode) Note(stringResource(R.string.kumi_demo_note))
                            }
                        }
                        if (state.connected && state.dacExtras != null) {
                            item {
                                PowerSleepCard(
                                    extras = state.dacExtras,
                                    onExtras = { kumi.setDacExtras(it) },
                                )
                            }
                        }
                        if (kind == CaseKind.KUMI) {
                            item {
                                Card(
                                    Modifier.fillMaxWidth(),
                                    shape = RoundedCornerShape(16.dp),
                                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
                                ) {
                                    Column(Modifier.padding(horizontal = 12.dp, vertical = 4.dp)) {
                                        NavRow(R.drawable.ic_memory, R.string.kumi_details_title, stringResource(R.string.kumi_details_subtitle), onOpenDetails, keyHint = page0Keys.label("details"))
                                    }
                                }
                            }
                        }
                    }
                    1 -> {
                        if (firmware.nativeMissing) item { Note(stringResource(R.string.kumi_native_missing), MaterialTheme.colorScheme.error) }
                        item {
                            UpdatesCenterCard(
                                firmware = firmware,
                                catalog = catalog,
                                kind = kind,
                                installed = installed,
                                installedBuild = installedBuild,
                                devChannel = devChannel,
                                ramOnly = state.ramOnly,
                                busy = false,
                                blocked = blocked,
                                demo = settings.demoMode,
                                repository = settings.releaseRepository,
                                installError = installError,
                                installNotice = installNotice,
                                onDismissInstallError = { installError = null },
                                onInstallPackage = { confirmInstall = it },
                                onUpdatePackage = { flows.update(it) },
                                onInstallApk = launchApkInstall,
                                onShowChangelog = { changelog = it },
                            )
                        }
                    }
                    else -> {
                        item {
                            SectionCard(stringResource(R.string.kumi_stock_title)) {
                                if (backupDate != null) {
                                    InfoRow(stringResource(R.string.kumi_backup_taken), backupDate)
                                } else {
                                    Note(stringResource(R.string.kumi_backup_none))
                                }
                                Note(stringResource(R.string.kumi_restore_text))
                                OutlinedButton(
                                    onClick = { confirmRestore = true },
                                    enabled = kind == CaseKind.KUMI,
                                ) { Text(stringResource(R.string.kumi_restore_action)) }
                            }
                        }
                    }
                }
            }
            KeyboardHintStrip(
                hints = if (page == 0 && kind == CaseKind.KUMI) {
                    listOf(page0Keys.label("details") to shDetails, "↵" to shTab, "⌫" to shBack)
                } else {
                    listOf("↵" to shTab, "⌫" to shBack)
                },
            )
        }
    }

    confirmInstall?.let { pkg ->
        ConfirmDialog(
            title = stringResource(R.string.kumi_install_confirm_title, pkg.version.toString()),
            text = stringResource(R.string.kumi_install_confirm_text),
            onConfirm = { flows.install(pkg) },
            onDismiss = { confirmInstall = null },
        )
    }
    if (confirmRestore) {
        ConfirmDialog(
            title = stringResource(R.string.kumi_restore_action),
            text = stringResource(R.string.kumi_restore_confirm_text),
            onConfirm = { flows.restoreStock(exact = true) },
            onDismiss = { confirmRestore = false },
        )
    }
    changelog?.let { preview ->
        ChangelogDialog(
            title = preview.title,
            subtitle = preview.subtitle,
            notes = preview.notes,
            releaseUrl = preview.releaseUrl,
            primaryLabel = preview.primaryLabel,
            onPrimary = preview.onPrimary,
            onDismiss = { changelog = null },
        )
    }
}

@Composable
private fun CompactUpgradeBenefitsCard() {
    val items = listOf(
        R.string.kumi_why_sleep_title,
        R.string.kumi_why_sound_title,
        R.string.kumi_why_keys_title,
        R.string.kumi_why_safe_title,
    )
    SectionCard(stringResource(R.string.kumi_why_title)) {
        items.forEach { titleRes ->
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Icon(
                    painterResource(R.drawable.ic_check),
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.size(16.dp),
                )
                Text(stringResource(titleRes), style = MaterialTheme.typography.bodyMedium)
            }
        }
        Note(stringResource(R.string.kumi_install_steps_body))
    }
}

@Composable
private fun UpdatesCenterCard(
    firmware: CatalogState,
    catalog: ReleaseCatalog,
    kind: CaseKind,
    installed: KumiVersion?,
    installedBuild: String?,
    devChannel: Boolean,
    ramOnly: Boolean,
    busy: Boolean,
    blocked: Boolean,
    demo: Boolean,
    repository: String,
    installError: String?,
    installNotice: String?,
    onDismissInstallError: () -> Unit,
    onInstallPackage: (FirmwarePackage) -> Unit,
    onUpdatePackage: (FirmwarePackage) -> Unit,
    onInstallApk: (File) -> Unit,
    onShowChangelog: (ChangelogPreview) -> Unit,
) {
    val resources = LocalResources.current
    val appUpdate = remember(firmware.appReleases) { firmware.appUpdate(BuildConfig.VERSION_NAME) }
    val latestApp = firmware.appReleases.firstOrNull()
    val otherAppReleases = remember(firmware.appReleases, appUpdate, devChannel) {
        if (!devChannel) emptyList()
        else firmware.appReleases.filter { it.asset != appUpdate?.asset }
    }
    val downloadableFw = remember(firmware.packages, firmware.releases) {
        val downloadedNames = firmware.packages.map { it.name }.toSet()
        firmware.releases.filter { it.asset !in downloadedNames }
    }
    val sortedPackages = remember(firmware.packages) {
        firmware.packages.sortedWith(
            compareByDescending<FirmwarePackage> { it.releaseVersion }.thenByDescending { it.info.generation },
        )
    }
    val installable = kind == CaseKind.STOCK || kind == CaseKind.SERVICE || (kind == CaseKind.KUMI && ramOnly)
    val bestFwPkg = remember(sortedPackages, kind, installed, installedBuild, installable) {
        if (installable) sortedPackages.firstOrNull()
        else if (kind == CaseKind.KUMI && installed != null) {
            sortedPackages.firstOrNull { FirmwareCatalog.isPackageNewerThan(it, installed, installedBuild) }
        } else null
    }
    val bestRemoteFw = remember(downloadableFw, kind, installed, installedBuild, bestFwPkg) {
        val candidate = if (kind == CaseKind.KUMI && installed != null) {
            downloadableFw.firstOrNull { FirmwareCatalog.isReleaseNewerThan(it, installed, installedBuild) }
        } else null
        if (candidate != null && (bestFwPkg == null || (candidate.releaseVersion != null && candidate.releaseVersion!! > bestFwPkg.releaseVersion))) {
            candidate
        } else null
    }
    val pendingFwVersion = bestRemoteFw?.displayVersion ?: bestFwPkg?.displayVersion
    val otherPackages = remember(sortedPackages, bestFwPkg) {
        sortedPackages.filter { it.file != bestFwPkg?.file }
    }
    val otherRemoteFw = remember(downloadableFw, bestRemoteFw) {
        downloadableFw.filter { it.asset != bestRemoteFw?.asset }
    }
    val otherCount = otherPackages.size + otherRemoteFw.size + otherAppReleases.size
    var showOtherVersions by rememberSaveable { mutableStateOf(false) }

    fun notesForPackage(pkg: FirmwarePackage): Pair<String, String?>? {
        val matchedFw = firmware.releases.firstOrNull { it.releaseVersion == pkg.releaseVersion }
            ?: firmware.releases.firstOrNull { it.version == pkg.version }
        val matchedApp = firmware.appReleases.firstOrNull { it.releaseVersion == pkg.releaseVersion }
            ?: firmware.appReleases.firstOrNull { it.version == pkg.version }
        val text = pkg.notes?.takeIf { it.isNotBlank() }
            ?: matchedFw?.notes?.takeIf { it.isNotBlank() }
            ?: matchedApp?.notes?.takeIf { it.isNotBlank() }
            ?: return null
        val url = matchedFw?.htmlUrl?.takeIf { it.isNotBlank() } ?: matchedApp?.htmlUrl?.takeIf { it.isNotBlank() }
        return text to url
    }

    SectionCard(
        title = stringResource(R.string.kumi_updates_title),
        trailing = {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                if (!demo && devChannel) {
                    StatusPill(
                        text = stringResource(R.string.settings_channel_dev),
                        containerColor = MaterialTheme.colorScheme.tertiaryContainer,
                        contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
                    )
                }
                if (!demo) {
                    TextButton(
                        onClick = { catalog.checkReleases() },
                        enabled = !firmware.checking,
                        contentPadding = PaddingValues(horizontal = 8.dp, vertical = 0.dp),
                    ) {
                        Text(stringResource(R.string.kumi_updates_check), style = MaterialTheme.typography.labelMedium)
                    }
                }
            }
        },
    ) {
        if (firmware.checking) LinearProgressIndicator(Modifier.fillMaxWidth())
        firmware.releaseError?.let { err ->
            DismissibleErrorBanner(friendlyNetworkError(err), onDismiss = { catalog.clearReleaseError() })
        }
        installError?.let { err ->
            DismissibleErrorBanner(err, onDismiss = onDismissInstallError)
        }
        installNotice?.let { notice ->
            Surface(
                color = MaterialTheme.colorScheme.tertiaryContainer,
                contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
                shape = RoundedCornerShape(12.dp),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(notice, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(10.dp))
            }
        }
        if (firmware.noReleases) Note(stringResource(R.string.kumi_releases_none))

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (!demo) {
                UpdateSummaryTile(
                    label = stringResource(R.string.kumi_updates_app_section),
                    version = "v${BuildConfig.VERSION_NAME}",
                    status = appUpdate?.let { "→ ${it.displayVersion}" } ?: stringResource(R.string.kumi_updates_up_to_date_short),
                    highlighted = appUpdate != null,
                    modifier = Modifier.weight(1f),
                )
            }
            UpdateSummaryTile(
                label = stringResource(R.string.kumi_updates_firmware_section),
                version = installed?.let { "v${ReleaseVersion.fromBuild(it, installedBuild)}" } ?: "—",
                status = when {
                    pendingFwVersion != null -> "→ $pendingFwVersion"
                    installed != null -> stringResource(R.string.kumi_updates_up_to_date_short)
                    else -> stringResource(R.string.kumi_case_absent)
                },
                highlighted = pendingFwVersion != null,
                modifier = Modifier.weight(1f),
            )
        }

        if (!demo && appUpdate != null) {
            AppReleaseRow(
                asset = appUpdate,
                downloaded = firmware.downloadedApks[appUpdate.asset],
                downloading = firmware.downloadingApp == appUpdate.asset,
                progress = firmware.appDownloadProgress,
                anyDownloading = firmware.downloadingApp != null,
                onDownload = { catalog.downloadApp(appUpdate, onReady = onInstallApk) },
                onInstall = onInstallApk,
                onShowChangelogWithCta = { label ->
                    val cached = firmware.downloadedApks[appUpdate.asset]
                    onShowChangelog(
                        ChangelogPreview(
                            title = appUpdate.title,
                            subtitle = releaseSubtitle(resources, appUpdate.published, appUpdate.size, appUpdate.prerelease),
                            notes = appUpdate.notes,
                            releaseUrl = appUpdate.htmlUrl.takeIf { it.isNotBlank() },
                            primaryLabel = label,
                            onPrimary = {
                                if (cached != null) onInstallApk(cached)
                                else catalog.downloadApp(appUpdate, onReady = onInstallApk)
                            },
                        ),
                    )
                },
            )
        }

        if (firmware.keyMissing) {
            Note(stringResource(R.string.kumi_key_missing), MaterialTheme.colorScheme.error)
        }

        if (bestRemoteFw != null && !demo && !firmware.keyMissing) {
            Surface(
                color = MaterialTheme.colorScheme.secondaryContainer,
                contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                shape = RoundedCornerShape(14.dp),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Column(Modifier.padding(10.dp)) {
                    DownloadableFirmwareRow(
                        release = bestRemoteFw,
                        downloading = firmware.downloading == bestRemoteFw.asset,
                        anyDownloading = firmware.downloading != null,
                        onDownload = { catalog.download(bestRemoteFw) },
                        onChangelog = { label ->
                            onShowChangelog(
                                ChangelogPreview(
                                    title = bestRemoteFw.title,
                                    subtitle = releaseSubtitle(resources, bestRemoteFw.published, bestRemoteFw.size, bestRemoteFw.prerelease),
                                    notes = bestRemoteFw.notes,
                                    releaseUrl = bestRemoteFw.htmlUrl.takeIf { it.isNotBlank() },
                                    primaryLabel = label,
                                    onPrimary = { catalog.download(bestRemoteFw) },
                                ),
                            )
                        },
                    )
                }
            }
        }

        if (bestFwPkg != null) {
            val pkgChangelog = notesForPackage(bestFwPkg)
            FirmwarePackageRow(
                pkg = bestFwPkg,
                kind = kind,
                installed = installed,
                installedBuild = installedBuild,
                devChannel = devChannel,
                installable = installable,
                busy = busy,
                blocked = blocked,
                onInstall = { onInstallPackage(bestFwPkg) },
                onUpdate = { onUpdatePackage(bestFwPkg) },
                onChangelog = pkgChangelog?.let { (notes, url) ->
                    {
                        onShowChangelog(
                            ChangelogPreview(
                                title = "Kumi ${bestFwPkg.displayVersion}",
                                subtitle = bestFwPkg.info.build,
                                notes = notes,
                                releaseUrl = url,
                            ),
                        )
                    }
                },
            )
        }

        if (!demo && appUpdate == null && bestFwPkg == null && bestRemoteFw == null && latestApp != null && latestApp.notes.isNotBlank()) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Note(stringResource(R.string.kumi_updates_app_up_to_date, BuildConfig.VERSION_NAME), modifier = Modifier.weight(1f))
                TextButton(
                    onClick = {
                        onShowChangelog(
                            ChangelogPreview(
                                title = latestApp.title,
                                subtitle = releaseSubtitle(resources, latestApp.published, latestApp.size, latestApp.prerelease),
                                notes = latestApp.notes,
                                releaseUrl = latestApp.htmlUrl.takeIf { it.isNotBlank() },
                            ),
                        )
                    },
                ) { Text(stringResource(R.string.kumi_updates_changelog)) }
            }
        }

        if (sortedPackages.isEmpty() && downloadableFw.isEmpty()) {
            Note(stringResource(R.string.kumi_no_packages))
        }

        if (otherCount > 0) {
            HorizontalDivider()
            Row(
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(10.dp))
                    .clickable { showOtherVersions = !showOtherVersions }
                    .padding(vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    stringResource(R.string.kumi_updates_other_versions, otherCount),
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f),
                )
                Text(
                    if (showOtherVersions) "▲" else "▼",
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            AnimatedVisibility(visible = showOtherVersions) {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (otherPackages.isNotEmpty() || (!demo && !firmware.keyMissing && otherRemoteFw.isNotEmpty())) {
                        Text(stringResource(R.string.kumi_updates_firmware_section), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
                        otherPackages.forEach { pkg ->
                            val pkgChangelog = notesForPackage(pkg)
                            FirmwarePackageRow(
                                pkg = pkg,
                                kind = kind,
                                installed = installed,
                                installedBuild = installedBuild,
                                devChannel = devChannel,
                                installable = installable,
                                busy = busy,
                                blocked = blocked,
                                onInstall = { onInstallPackage(pkg) },
                                onUpdate = { onUpdatePackage(pkg) },
                                onChangelog = pkgChangelog?.let { (notes, url) ->
                                    {
                                        onShowChangelog(
                                            ChangelogPreview(
                                                title = "Kumi ${pkg.displayVersion}",
                                                subtitle = pkg.info.build,
                                                notes = notes,
                                                releaseUrl = url,
                                            ),
                                        )
                                    }
                                },
                            )
                        }
                        if (!demo && !firmware.keyMissing) {
                            otherRemoteFw.forEach { release ->
                                DownloadableFirmwareRow(
                                    release = release,
                                    downloading = firmware.downloading == release.asset,
                                    anyDownloading = firmware.downloading != null,
                                    onDownload = { catalog.download(release) },
                                    onChangelog = { label ->
                                        onShowChangelog(
                                            ChangelogPreview(
                                                title = release.title,
                                                subtitle = releaseSubtitle(resources, release.published, release.size, release.prerelease),
                                                notes = release.notes,
                                                releaseUrl = release.htmlUrl.takeIf { it.isNotBlank() },
                                                primaryLabel = label,
                                                onPrimary = { catalog.download(release) },
                                            ),
                                        )
                                    },
                                )
                            }
                        }
                    }
                    if (!demo && otherAppReleases.isNotEmpty()) {
                        HorizontalDivider()
                        Text(stringResource(R.string.kumi_updates_app_section), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
                        otherAppReleases.forEach { asset ->
                            DevAppReleaseRow(
                                asset = asset,
                                downloaded = firmware.downloadedApks[asset.asset],
                                downloading = firmware.downloadingApp == asset.asset,
                                progress = firmware.appDownloadProgress,
                                anyDownloading = firmware.downloadingApp != null,
                                onDownload = { catalog.downloadApp(asset, onReady = onInstallApk) },
                                onInstall = onInstallApk,
                                onShowChangelogWithCta = { label ->
                                    val cached = firmware.downloadedApks[asset.asset]
                                    onShowChangelog(
                                        ChangelogPreview(
                                            title = asset.title,
                                            subtitle = releaseSubtitle(resources, asset.published, asset.size, asset.prerelease),
                                            notes = asset.notes,
                                            releaseUrl = asset.htmlUrl.takeIf { it.isNotBlank() },
                                            primaryLabel = label,
                                            onPrimary = {
                                                if (cached != null) onInstallApk(cached)
                                                else catalog.downloadApp(asset, onReady = onInstallApk)
                                            },
                                        ),
                                    )
                                },
                            )
                        }
                    }
                }
            }
        }

        if (!demo) {
            Note(stringResource(R.string.kumi_releases_repo, repository.ifBlank { BuildConfig.KUMI_RELEASES_REPO }))
        }
    }
}

@Composable
private fun UpdateSummaryTile(
    label: String,
    version: String,
    status: String,
    highlighted: Boolean,
    modifier: Modifier = Modifier,
) {
    val bg = if (highlighted) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh
    val fg = if (highlighted) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurface
    Surface(
        color = bg,
        contentColor = fg,
        shape = RoundedCornerShape(12.dp),
        modifier = modifier,
    ) {
        Column(Modifier.padding(horizontal = 10.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
                Text(label, style = MaterialTheme.typography.labelSmall, color = fg.copy(alpha = 0.78f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(version, style = MaterialTheme.typography.labelSmall, fontWeight = FontWeight.SemiBold, maxLines = 1)
            }
            Text(
                status,
                style = MaterialTheme.typography.labelMedium,
                fontWeight = if (highlighted) FontWeight.SemiBold else FontWeight.Medium,
                color = if (highlighted) MaterialTheme.colorScheme.primary else fg.copy(alpha = 0.78f),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun DevAppReleaseRow(
    asset: AppReleaseAsset,
    downloaded: File?,
    downloading: Boolean,
    progress: Float?,
    anyDownloading: Boolean,
    onDownload: () -> Unit,
    onInstall: (File) -> Unit,
    onShowChangelogWithCta: (String) -> Unit,
) {
    val ctaLabel = stringResource(if (downloaded != null) R.string.kumi_updates_install_now else R.string.kumi_updates_download_install)
    val isCurrent = asset.displayVersion.equals(BuildConfig.VERSION_NAME, ignoreCase = true)
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(asset.title, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            if (asset.prerelease) {
                EvidenceLabel(stringResource(R.string.kumi_prerelease))
            }
        }
        Note(
            listOfNotNull(
                asset.displayVersion,
                stringResource(R.string.kumi_updates_size_mb, asset.size / (1024.0 * 1024.0)),
                asset.published.take(10).takeIf { it.isNotBlank() },
            ).joinToString(" · "),
        )
        if (downloading) {
            val pct = ((progress ?: 0f) * 100f).toInt().coerceIn(0, 100)
            Text(stringResource(R.string.kumi_updates_downloading, pct), style = MaterialTheme.typography.bodySmall)
            if (progress != null) LinearProgressIndicator(progress = { progress }, modifier = Modifier.fillMaxWidth())
            else LinearProgressIndicator(Modifier.fillMaxWidth())
        } else {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                if (isCurrent) {
                    Note(stringResource(R.string.kumi_installed_now))
                } else {
                    FilledTonalButton(
                        onClick = { if (downloaded != null) onInstall(downloaded) else onDownload() },
                        enabled = !anyDownloading,
                    ) { Text(ctaLabel) }
                }
                if (asset.notes.isNotBlank()) {
                    OutlinedButton(onClick = { onShowChangelogWithCta(ctaLabel) }) {
                        Text(stringResource(R.string.kumi_updates_changelog))
                    }
                }
            }
        }
    }
}

@Composable
private fun AppReleaseRow(
    asset: AppReleaseAsset,
    downloaded: File?,
    downloading: Boolean,
    progress: Float?,
    anyDownloading: Boolean,
    onDownload: () -> Unit,
    onInstall: (File) -> Unit,
    onShowChangelogWithCta: (String) -> Unit,
) {
    val ctaLabel = stringResource(if (downloaded != null) R.string.kumi_updates_install_now else R.string.kumi_updates_download_install)
    val targetVersion = asset.displayVersion
    Surface(
        color = MaterialTheme.colorScheme.primaryContainer,
        contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
        shape = RoundedCornerShape(14.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(stringResource(R.string.kumi_updates_app_available, targetVersion), style = MaterialTheme.typography.titleSmall)
                    Text(
                        listOfNotNull(
                            stringResource(R.string.kumi_updates_version_arrow, BuildConfig.VERSION_NAME, targetVersion),
                            stringResource(R.string.kumi_updates_size_mb, asset.size / (1024.0 * 1024.0)),
                            stringResource(R.string.kumi_prerelease).takeIf { asset.prerelease },
                        ).joinToString(" · "),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.85f),
                    )
                }
                StatusPill(
                    text = stringResource(R.string.kumi_updates_badge_new),
                    containerColor = MaterialTheme.colorScheme.primary,
                    contentColor = MaterialTheme.colorScheme.onPrimary,
                )
            }
            if (downloading) {
                val pct = ((progress ?: 0f) * 100f).toInt().coerceIn(0, 100)
                Text(stringResource(R.string.kumi_updates_downloading, pct), style = MaterialTheme.typography.bodySmall)
                if (progress != null) LinearProgressIndicator(progress = { progress }, modifier = Modifier.fillMaxWidth())
                else LinearProgressIndicator(Modifier.fillMaxWidth())
            } else {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(
                        onClick = { if (downloaded != null) onInstall(downloaded) else onDownload() },
                        enabled = !anyDownloading,
                    ) { Text(ctaLabel) }
                    OutlinedButton(onClick = { onShowChangelogWithCta(ctaLabel) }) {
                        Text(stringResource(R.string.kumi_updates_changelog))
                    }
                }
            }
        }
    }
}

@Composable
private fun FirmwarePackageRow(
    pkg: FirmwarePackage,
    kind: CaseKind,
    installed: KumiVersion?,
    installedBuild: String?,
    devChannel: Boolean,
    installable: Boolean,
    busy: Boolean,
    blocked: Boolean,
    onInstall: () -> Unit,
    onUpdate: () -> Unit,
    onChangelog: (() -> Unit)?,
) {
    val isNewer = kind == CaseKind.KUMI && installed != null && FirmwareCatalog.isPackageNewerThan(pkg, installed, installedBuild)
    val isSameRunning = kind == CaseKind.KUMI && installed == pkg.version &&
        (installedBuild.isNullOrBlank() || installedBuild == pkg.info.build)
    val content: @Composable () -> Unit = {
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(
                    if (isNewer) "${stringResource(R.string.kumi_updates_firmware_section)} · ${pkg.displayVersion}"
                    else stringResource(R.string.kumi_name_version, pkg.displayVersion),
                    style = if (isNewer) MaterialTheme.typography.titleSmall else MaterialTheme.typography.bodyLarge,
                    modifier = Modifier.weight(1f),
                )
                if (isNewer) {
                    StatusPill(
                        text = stringResource(R.string.kumi_updates_badge_new),
                        containerColor = MaterialTheme.colorScheme.primary,
                        contentColor = MaterialTheme.colorScheme.onPrimary,
                    )
                }
                EvidenceLabel(stringResource(sourceName(pkg.source)))
            }
            Note(
                listOfNotNull(
                    installed?.takeIf { isNewer }?.let { stringResource(R.string.kumi_updates_version_arrow, it.toString(), pkg.displayVersion) },
                    stringResource(R.string.kumi_package_size, (pkg.info.stage0 + pkg.info.image) / 1024),
                    stringResource(R.string.kumi_verified),
                    pkg.info.build.takeIf { devChannel && it.isNotBlank() },
                ).joinToString(" · "),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                when {
                    installable && blocked -> Note(stringResource(R.string.kumi_install_not_enabled), MaterialTheme.colorScheme.error)
                    installable -> Button(onClick = onInstall, enabled = !busy) { Text(stringResource(R.string.kumi_install_action)) }
                    isNewer -> Button(onClick = onUpdate, enabled = !busy) {
                        Text(stringResource(R.string.kumi_update_to, pkg.displayVersion))
                    }
                    isSameRunning -> Note(stringResource(R.string.kumi_installed_now))
                    devChannel && kind == CaseKind.KUMI -> FilledTonalButton(onClick = onUpdate, enabled = !busy) {
                        Text(stringResource(R.string.kumi_flash_action))
                    }
                    else -> Unit
                }
                if (onChangelog != null) {
                    OutlinedButton(onClick = onChangelog) { Text(stringResource(R.string.kumi_updates_changelog)) }
                }
            }
        }
    }
    if (isNewer) {
        Surface(
            color = MaterialTheme.colorScheme.secondaryContainer,
            contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
            shape = RoundedCornerShape(14.dp),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Column(Modifier.padding(10.dp)) { content() }
        }
    } else {
        content()
    }
}

@Composable
private fun DownloadableFirmwareRow(
    release: ReleaseAsset,
    downloading: Boolean,
    anyDownloading: Boolean,
    onDownload: () -> Unit,
    onChangelog: (String) -> Unit,
) {
    val dlLabel = stringResource(R.string.kumi_download)
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(release.title, style = MaterialTheme.typography.bodyLarge)
        Note(
            listOfNotNull(
                stringResource(R.string.kumi_package_size, (release.size / 1024L).toInt().coerceAtLeast(1)),
                release.published.take(10).takeIf { it.isNotBlank() },
                stringResource(R.string.kumi_prerelease).takeIf { release.prerelease },
            ).joinToString(" · "),
        )
        if (downloading) {
            LinearProgressIndicator(Modifier.fillMaxWidth())
        } else {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                FilledTonalButton(onClick = onDownload, enabled = !anyDownloading) { Text(dlLabel) }
                OutlinedButton(onClick = { onChangelog(dlLabel) }) { Text(stringResource(R.string.kumi_updates_changelog)) }
            }
        }
    }
}

private fun releaseSubtitle(resources: android.content.res.Resources, published: String, sizeBytes: Long, prerelease: Boolean): String {
    val date = published.take(10).takeIf { it.isNotBlank() }
    val size = if (sizeBytes >= 1024L * 1024L) {
        resources.getString(R.string.kumi_updates_size_mb, sizeBytes / (1024.0 * 1024.0))
    } else {
        resources.getString(R.string.kumi_package_size, (sizeBytes / 1024L).toInt().coerceAtLeast(1))
    }
    return listOfNotNull(date, size, if (prerelease) resources.getString(R.string.kumi_prerelease) else null).joinToString(" · ")
}

@Composable
private fun EvidenceLabel(text: String) {
    Surface(color = MaterialTheme.colorScheme.secondaryContainer, shape = MaterialTheme.shapes.small) {
        Text(text, Modifier.padding(horizontal = 6.dp, vertical = 2.dp), style = MaterialTheme.typography.labelSmall)
    }
}

private fun sourceName(source: FirmwareSource): Int = when (source) {
    FirmwareSource.BUNDLED -> R.string.kumi_source_bundled
    FirmwareSource.RELEASE -> R.string.kumi_source_release
    FirmwareSource.DEMO -> R.string.kumi_source_demo
}

@Composable
private fun FlowCard(flow: FlowState, onResume: () -> Unit, onClose: () -> Unit, onOpenHome: () -> Unit) {
    val failed = flow.step == FlowStep.FAILED
    val done = flow.step == FlowStep.DONE
    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(20.dp),
        colors = CardDefaults.cardColors(
            containerColor = when {
                failed -> MaterialTheme.colorScheme.errorContainer
                done -> MaterialTheme.colorScheme.primaryContainer
                else -> MaterialTheme.colorScheme.secondaryContainer
            },
        ),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(
                stringResource(flowTitle(flow.kind), flow.target?.toString() ?: ""),
                style = MaterialTheme.typography.titleMedium,
            )
            val activeStepOrdinal = if (failed) flow.failedAt?.ordinal else flow.step.ordinal
            flowSteps(flow).forEach { step ->
                val completed = done || (activeStepOrdinal != null && step.ordinal < activeStepOrdinal)
                val current = step == flow.step && !done && !failed
                val failedHere = failed && step == flow.failedAt
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(
                        painterResource(if (completed) R.drawable.ic_check else R.drawable.ic_more),
                        contentDescription = null,
                        tint = when {
                            failedHere -> MaterialTheme.colorScheme.error
                            completed || current -> MaterialTheme.colorScheme.primary
                            else -> MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.55f)
                        },
                        modifier = Modifier.size(18.dp),
                    )
                    Spacer(Modifier.width(8.dp))
                    Text(
                        stringResource(stepName(step)),
                        style = if (current || failedHere) MaterialTheme.typography.bodyLarge else MaterialTheme.typography.bodyMedium,
                        color = when {
                            failedHere -> MaterialTheme.colorScheme.error
                            completed || current -> MaterialTheme.colorScheme.onSurface
                            else -> MaterialTheme.colorScheme.onSurfaceVariant
                        },
                    )
                }
            }
            if (!failed && !done) {
                val progress = flow.progress
                if (progress != null) LinearProgressIndicator(progress = { progress }, modifier = Modifier.fillMaxWidth())
                else LinearProgressIndicator(Modifier.fillMaxWidth())
                flow.label?.let { Note(it) }
                val serviceModeBusy = (flow.kind == FlowKind.INSTALL || flow.kind == FlowKind.RESTORE) &&
                    flow.step in setOf(FlowStep.ENTERING_SERVICE, FlowStep.BACKING_UP, FlowStep.WRITING)
                if (serviceModeBusy) {
                    Surface(
                        Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(14.dp),
                        color = MaterialTheme.colorScheme.tertiaryContainer,
                    ) {
                        Row(
                            Modifier.padding(12.dp),
                            verticalAlignment = Alignment.Top,
                            horizontalArrangement = Arrangement.spacedBy(10.dp),
                        ) {
                            Icon(
                                painterResource(R.drawable.ic_keyboard),
                                contentDescription = null,
                                tint = MaterialTheme.colorScheme.onTertiaryContainer,
                                modifier = Modifier.padding(top = 2.dp).size(20.dp),
                            )
                            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                                Text(
                                    stringResource(R.string.kumi_flow_tap_key_title),
                                    style = MaterialTheme.typography.titleSmall,
                                    color = MaterialTheme.colorScheme.onTertiaryContainer,
                                )
                                Text(
                                    stringResource(R.string.kumi_flow_tap_key_body),
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onTertiaryContainer,
                                )
                            }
                        }
                    }
                }
                Note(stringResource(R.string.kumi_flow_keep_attached))
            }
            if (done) Text(stringResource(doneText(flow.kind)), style = MaterialTheme.typography.bodyMedium)
            if (failed) {
                Text(flow.error ?: "", style = MaterialTheme.typography.bodyMedium)
                flow.failure?.let { failure ->
                    failure.label?.let { Note(stringResource(R.string.kumi_flow_failed_at, it)) }
                    val boots = listOfNotNull(failure.bootsBefore, failure.bootsAfter).distinct()
                    if (boots.isNotEmpty()) Note(stringResource(R.string.kumi_flow_boots, boots.map { bootName(it) }.joinToString(" / ")))
                }
                flow.notice?.let { Text(stringResource(noticeText(it)), style = MaterialTheme.typography.bodyLarge) }
                if (flow.replug) Note(stringResource(R.string.kumi_flow_replug))
                Note(stringResource(R.string.kumi_flow_safe))
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (done && flow.kind == FlowKind.INSTALL) {
                    Button(onClick = onOpenHome) { Text(stringResource(R.string.kumi_flow_open_kumi)) }
                }
                if (failed && flow.resumable) Button(onClick = onResume) { Text(stringResource(R.string.kumi_flow_resume)) }
                if (failed || done) TextButton(onClick = onClose) { Text(stringResource(R.string.action_close)) }
            }
        }
    }
}

@Composable
private fun bootName(name: String): String = stringResource(
    when (name) {
        "Stock" -> R.string.kumi_boots_stock
        "Kumi" -> R.string.kumi_boots_kumi
        "Service" -> R.string.kumi_boots_service
        else -> R.string.kumi_boots_unchanged
    },
)

private fun flowSteps(flow: FlowState): List<FlowStep> = when (flow.kind) {
    FlowKind.INSTALL -> listOf(FlowStep.ENTERING_SERVICE, FlowStep.BACKING_UP, FlowStep.WRITING, FlowStep.RESTARTING)
    FlowKind.UPDATE -> listOf(FlowStep.WRITING, FlowStep.RESTARTING, FlowStep.CONFIRMING)
    FlowKind.RESTORE -> if (flow.step == FlowStep.BACKING_UP || flow.failedAt == FlowStep.BACKING_UP) {
        listOf(FlowStep.ENTERING_SERVICE, FlowStep.BACKING_UP, FlowStep.WRITING, FlowStep.RESTARTING)
    } else {
        listOf(FlowStep.ENTERING_SERVICE, FlowStep.WRITING, FlowStep.RESTARTING)
    }
    FlowKind.RETURN -> listOf(FlowStep.RESTARTING)
}

private fun noticeText(notice: FlowNotice): Int = when (notice) {
    FlowNotice.INSTALL_NOT_ENABLED -> R.string.kumi_install_not_enabled
    FlowNotice.BACKUP_STALE -> R.string.kumi_backup_stale
}

private fun flowTitle(kind: FlowKind): Int = when (kind) {
    FlowKind.INSTALL -> R.string.kumi_flow_install
    FlowKind.UPDATE -> R.string.kumi_flow_update
    FlowKind.RESTORE -> R.string.kumi_flow_restore
    FlowKind.RETURN -> R.string.kumi_flow_return
}

private fun doneText(kind: FlowKind): Int = when (kind) {
    FlowKind.INSTALL -> R.string.kumi_flow_install_done
    FlowKind.UPDATE -> R.string.kumi_flow_update_done
    FlowKind.RESTORE -> R.string.kumi_flow_restore_done
    FlowKind.RETURN -> R.string.kumi_flow_return_done
}

private fun stepName(step: FlowStep): Int = when (step) {
    FlowStep.PREPARING -> R.string.kumi_step_preparing
    FlowStep.ENTERING_SERVICE -> R.string.kumi_step_service
    FlowStep.BACKING_UP -> R.string.kumi_step_backup
    FlowStep.WRITING -> R.string.kumi_step_writing
    FlowStep.RESTARTING -> R.string.kumi_step_restart
    FlowStep.CONFIRMING -> R.string.kumi_step_confirm
    FlowStep.DONE -> R.string.kumi_step_done
    FlowStep.FAILED -> R.string.kumi_step_failed
}
