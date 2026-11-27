// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import java.net.URL
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ReleaseHostsTest {
    @Test
    fun onlyGithubOverHttpsIsContacted() {
        assertTrue(ReleaseHosts.allowed(URL("https://api.github.com/repos/o/kumi/releases"), json = true))
        assertFalse(ReleaseHosts.allowed(URL("https://github.com/o/kumi/releases"), json = true))
        assertTrue(ReleaseHosts.download("https://github.com/o/kumi/releases/download/v0.1.0/kumi-0.1.0.kumi"))
        assertTrue(ReleaseHosts.download("https://objects.githubusercontent.com/github-production-release-asset/1"))
        assertTrue(ReleaseHosts.download("https://release-assets.githubusercontent.com/github-production-release-asset/1"))
        assertFalse(ReleaseHosts.download("http://github.com/o/kumi/k.kumi"))
        assertFalse(ReleaseHosts.download("https://github.com.example.org/k.kumi"))
        assertFalse(ReleaseHosts.download("https://user@github.com/k.kumi"))
        assertFalse(ReleaseHosts.download("https://github.com:8443/k.kumi"))
        assertFalse(ReleaseHosts.download("https://api.github.com/repos/o/kumi"))
        assertFalse(ReleaseHosts.download("not a url"))
        assertTrue(ReleaseHosts.webUrl("https://github.com/o/kumi/releases/tag/v1.1.0"))
        assertTrue(ReleaseHosts.webUrl("https://github.com/o/kumi/compare/v1.0.1...v1.1.0"))
        assertFalse(ReleaseHosts.webUrl("http://github.com/o/kumi/releases"))
        assertFalse(ReleaseHosts.webUrl("https://evil.example.com/o/kumi"))
        assertFalse(ReleaseHosts.webUrl("https://user@github.com/o/kumi"))
    }
}
