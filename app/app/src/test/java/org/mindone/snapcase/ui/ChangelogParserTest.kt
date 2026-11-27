// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ChangelogParserTest {
    @Test
    fun parsesStructuredMarkdownAndStripsRawCompareLine() {
        val compareUrl = "https://github.com/iasnezhkov/kumi/compare/v1.0.1...v1.1.0-rc5"
        val raw = listOf(
            "## Audio & DSP",
            "### Firmware",
            "- **Live RTA:** 24-band spectrum",
            "  - Sub-item with `code`",
            "> Licensed under PolyForm.",
            "---",
            "**Full Changelog**: $compareUrl",
        ).joinToString("\n")
        val blocks = parseChangelogBlocks(raw)
        assertEquals(6, blocks.size)
        assertEquals(ChangelogBlock.Heading("Audio & DSP", 2), blocks[0])
        assertEquals(ChangelogBlock.Heading("Firmware", 3), blocks[1])
        assertEquals(ChangelogBlock.Bullet("**Live RTA:** 24-band spectrum", 0), blocks[2])
        assertEquals(ChangelogBlock.Bullet("Sub-item with `code`", 1), blocks[3])
        assertEquals(ChangelogBlock.Quote("Licensed under PolyForm."), blocks[4])
        assertTrue(blocks[5] is ChangelogBlock.Divider)
        assertEquals(compareUrl, extractChangelogUrl(raw, null))
    }

    @Test
    fun prefersSafeReleaseHtmlUrlAndRejectsUntrustedHosts() {
        val compareUrl = "https://github.com/iasnezhkov/kumi/compare/v1.0.1...v1.1.0-rc5"
        val releaseUrl = "https://github.com/iasnezhkov/kumi/releases/tag/v1.1.0-rc5"
        val notes = "**Full Changelog**: $compareUrl"
        assertEquals(releaseUrl, extractChangelogUrl(notes, releaseUrl))
        assertNull(extractChangelogUrl("No links here", "https://evil.example.com/phish"))
    }
}
