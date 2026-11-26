// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import org.mindone.snapcase.transport.CaseKind

enum class HomeKind { STOCK, KUMI }

object Routing {
    fun home(kind: CaseKind, kumiSeen: Boolean): HomeKind = when (kind) {
        CaseKind.KUMI -> HomeKind.KUMI
        CaseKind.STOCK -> HomeKind.STOCK
        CaseKind.SERVICE, CaseKind.NONE -> if (kumiSeen) HomeKind.KUMI else HomeKind.STOCK
    }

    fun legacyVisible(kind: CaseKind, kumiSeen: Boolean, showLegacy: Boolean): Boolean =
        showLegacy || home(kind, kumiSeen) == HomeKind.STOCK

    fun offerInstall(kind: CaseKind, available: KumiVersion?, dismissed: String?): Boolean =
        kind == CaseKind.STOCK && available != null && dismissed != available.toString()
}
