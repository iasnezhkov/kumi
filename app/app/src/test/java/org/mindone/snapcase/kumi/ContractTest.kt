// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import java.io.File
import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import org.mindone.snapcase.transport.CaseUsb
import org.mindone.snapcase.transport.UsbId

class ContractTest {
    private val fixture = JSONObject(File(System.getProperty("kumi.contract") ?: "../../kumi/crates/kumi-jni/tests/fixtures/contract.json").readText())

    @Test
    fun deviceInfoAndAudioStatusParse() {
        val info = DeviceInfo.from(fixture.getJSONObject("device_info"))
        assertEquals(KumiVersion(0, 1, 2), info.firmware)
        assertEquals("ci-17", info.build)
        assertEquals(0x4901_4480L, info.chipId)
        assertEquals(listOf(0xC8, 0x65, 0x16), info.flashJedec)
        assertEquals("SN000001", info.serialText)
        assertTrue(info.capabilities.firmwareUpdate && info.capabilities.parametricEq && !info.capabilities.developer)
        assertEquals(0L, info.capabilities.otherBits)

        val unset = DeviceInfo.from(fixture.getJSONObject("device_info_unset_serial"))
        assertNull(unset.serial)
        assertNull(unset.serialText)
        assertTrue(unset.runningFromRam && unset.capabilities.keymap && unset.capabilities.layers && !unset.capabilities.parametricEq)
        val future = JSONObject(fixture.getJSONObject("device_info").toString())
        future.getJSONObject("capabilities").put("other_bits", 4096).remove("battery")
        val tolerated = DeviceInfo.from(future).capabilities
        assertEquals(4096L, tolerated.otherBits)
        assertFalse(tolerated.battery)

        val audio = AudioStatus.from(fixture.getJSONObject("audio_status"))
        assertEquals(Jack.Headset, audio.jack)
        assertEquals(StreamFormat(StreamKind.Dop, 176_400, 24, 2), audio.stream)
        assertEquals(listOf(-600, -650), audio.hostVolumeCentiDb)
        assertEquals(1, audio.eqActive)
        assertEquals(12L, audio.counters.feedbackCorrections)

        val idle = AudioStatus.from(fixture.getJSONObject("audio_status_idle"))
        assertNull(idle.stream)
        assertNull(idle.eqActive)
        assertTrue(idle.hostMuted)
    }

    @Test
    fun requestsEncodeExactlyAsTheBridgeExpects() {
        val output = OutputSettings.from(fixture.getJSONObject("output"))
        assertEquals(OutputFilter.SlowPhaseCompensated, output.filter)
        assertTrue(same(output.toJson(), fixture.getJSONObject("output")))

        val dsp = DspSettings.from(fixture.getJSONObject("dsp"))
        assertEquals(CrossfeedMode.Natural, dsp.crossfeed)
        assertTrue(dsp.loudness && !dsp.swapChannels)
        assertTrue(same(dsp.toJson(), fixture.getJSONObject("dsp")))

        val extras = DacExtras.from(fixture.getJSONObject("dac_extras"))
        assertEquals(ClassHMode.LowPowerLv, extras.classH)
        assertTrue(same(extras.toJson(), fixture.getJSONObject("dac_extras")))

        val toneReq = TestToneRequest.from(fixture.getJSONObject("test_tone_request"))
        assertTrue(toneReq.valid && toneReq.invertRight)
        assertTrue(same(toneReq.toJson(), fixture.getJSONObject("test_tone_request")))

        val keyCfg = KeyConfig.from(fixture.getJSONObject("key_config"))
        assertTrue(keyCfg.valid && keyCfg.comboTermMs == 50)
        assertTrue(same(keyCfg.toJson(), fixture.getJSONObject("key_config")))

        val imp = ImpedanceReport.from(Replies.caseResult(fixture.getJSONObject("impedance_report")) as JSONObject)
        assertEquals(49, imp.leftOhms)
        assertEquals(53, imp.leftAcOhms)
        assertEquals(53, imp.rightAcOhms)
        val meters = AudioMeters.from(fixture.getJSONObject("audio_meters"))
        assertTrue(meters.active && meters.limiterHits == 3L)
        val telemetry = SystemTelemetry.from(Replies.caseResult(fixture.getJSONObject("system_telemetry")) as JSONObject)
        assertEquals(90, telemetry.cpuMhz)
        assertTrue(telemetry.dcdcActive)

        val profile = EqProfile.from(fixture.getJSONObject("eq_profile"))
        assertEquals(2, profile.bands.size)
        assertEquals(BandKind.HighPass, profile.bands[1].kind)
        assertTrue(same(profile.toJson(), fixture.getJSONObject("eq_profile")))
        assertTrue(same(MicSettings.from(fixture.getJSONObject("mic")).toJson(), fixture.getJSONObject("mic")))

        val chunk = KeymapChunk.from(fixture.getJSONObject("keymap_chunk"))
        assertTrue(chunk.more)
        assertEquals(
            listOf(
                KeyAction.Transparent, KeyAction.Disabled, KeyAction.Key(4), KeyAction.Modifiers(0x22), KeyAction.KeyWithModifiers(5, 1),
                KeyAction.Consumer(0xE9), KeyAction.LayerHold(1), KeyAction.LayerToggle(3),
                KeyAction.TapHoldMod(0x29, 1), KeyAction.TapHoldLayer(0x2C, 1), KeyAction.OneShotMod(2),
                KeyAction.Macro(0), KeyAction.CaseControl(CaseHotkey.CycleEq), KeyAction.CapsWord, KeyAction.Mouse(MouseAction.LeftClick),
            ),
            chunk.entries.map { it.action },
        )
        val entries = fixture.getJSONObject("keymap_chunk").getJSONArray("entries")
        chunk.entries.forEachIndexed { i, entry -> assertTrue("entry $i", same(entry.toJson(), entries.getJSONObject(i))) }
    }

    @Test
    fun everyRequestTheClientSendsMatchesTheBridgeFixture() {
        val bridge = RecordingBridge()
        val client = KumiClient(bridge, NoLink)
        val profile = EqProfile.from(fixture.getJSONObject("eq_profile"))

        client.setEq(2, profile)
        assertTrue(same(bridge.last("audio/eq/set"), fixture.get("eq_update")))
        client.selectEq(null)
        assertTrue(same(bridge.last("audio/eq/select"), fixture.get("eq_selection_none")))
        client.selectEq(3)
        assertTrue(same(bridge.last("audio/eq/select"), fixture.get("eq_selection_some")))
        client.keymap(1)
        assertTrue(same(bridge.last("keys/map/get"), fixture.get("keymap_page")))
        client.devPeek(0x0010_2000, 4)
        assertTrue(same(bridge.last("dev/peek"), fixture.get("peek")))
        client.devUnlock(300)
        assertTrue(same(bridge.last("dev/unlock"), fixture.get("unlock")))
        val poke = fixture.getJSONObject("poke")
        assertEquals(0x1A5L, client.devPoke(poke.getLong("address"), poke.getLong("value"), poke.getLong("mask")))
        assertTrue(same(bridge.last("dev/poke"), poke))
        val i2c = fixture.getJSONObject("i2c_request")
        val reply = client.devI2c(i2c.getInt("bus"), i2c.getInt("address"), i2c.getJSONArray("write").ints(), i2c.getInt("read"))
        assertTrue(same(bridge.last("dev/i2c"), i2c))
        assertEquals(I2cReply(true, listOf(0x03, 0x41)), reply)
        val tone = AudioStart(48_000, AudioChannel.Left, -2_000, 1_000)
        val benchRequests = listOf(AudioBenchRequest.Calibrate, AudioBenchRequest.Start(tone), AudioBenchRequest.Stop, AudioBenchRequest.Status)
        benchRequests.forEachIndexed { i, request ->
            client.audioBench(request)
            assertTrue("bench request $i", same(bridge.last("dev/audio"), fixture.getJSONArray("audio_bench_requests").get(i)))
        }
        client.setOutput(OutputSettings.from(fixture.getJSONObject("output")))
        assertTrue(same(bridge.last("audio/output/set"), fixture.get("output")))
        client.setDsp(DspSettings.from(fixture.getJSONObject("dsp")))
        assertTrue(same(bridge.last("audio/dsp/set"), fixture.get("dsp")))
        client.setDacExtras(DacExtras.from(fixture.getJSONObject("dac_extras")))
        assertTrue(same(bridge.last("audio/dac-extras/set"), fixture.get("dac_extras")))
        client.playTestTone(TestToneRequest.from(fixture.getJSONObject("test_tone_request")))
        assertTrue(same(bridge.last("audio/test-tone"), fixture.get("test_tone_request")))
        client.setKeyConfig(KeyConfig.from(fixture.getJSONObject("key_config")))
        assertTrue(same(bridge.last("keys/config/set"), fixture.get("key_config")))
        val macroUpd = fixture.getJSONObject("macro_slot_update")
        client.setMacro(macroUpd.getInt("slot"), MacroProgram.from(macroUpd.getJSONObject("program")))
        assertTrue(same(bridge.last("keys/macro/set"), macroUpd))
        assertNull(client.macroSlot(1))
        val comboUpd = fixture.getJSONObject("combo_slot_update")
        client.setCombo(comboUpd.getInt("slot"), KeyCombo.from(comboUpd.getJSONObject("combo")))
        assertTrue(same(bridge.last("keys/combo/set"), comboUpd))
        assertNull(client.comboSlot(1))
        client.setMic(MicSettings.from(fixture.getJSONObject("mic")))
        assertTrue(same(bridge.last("audio/mic/set"), fixture.get("mic")))
        val entries = fixture.getJSONObject("keymap_chunk").getJSONArray("entries")
        KeymapChunk.from(fixture.getJSONObject("keymap_chunk")).entries.forEachIndexed { i, entry ->
            client.setKey(entry)
            assertTrue("entry $i", same(bridge.last("keys/map/set"), entries.get(i)))
        }
        RebootTarget.entries.forEach { target ->
            client.reboot(target)
            assertEquals(target.name, bridge.last("system/reboot"))
        }
        SettingsScope.entries.forEach { scope ->
            client.resetSettings(scope)
            assertEquals(scope.name, bridge.last("settings/reset"))
        }
        assertEquals(EventMask(keys = true, log = false, audio = false, uart = false), client.subscribe(EventMask(keys = true, log = true, audio = false, uart = false)))
        assertTrue(same(bridge.last("events/subscribe"), fixture.get("event_mask")))
        assertNull(client.eq(1))
        client.clearKeymap()
        client.confirmBoot()
        assertEquals(JSONObject.NULL, bridge.last("keys/map/clear"))
        assertEquals(JSONObject.NULL, bridge.last("system/confirm"))
        val paths = fixture.getJSONArray("endpoints").strings().toSet()
        assertTrue((bridge.paths - paths).toString(), paths.containsAll(bridge.paths))
    }

    @Test
    fun enumsAndResultsMatchTheRustNames() {
        assertEquals(OutputFilter.entries.map { it.name }, fixture.getJSONArray("dac_filters").strings())
        assertEquals(RebootTarget.entries.map { it.name }, fixture.getJSONArray("reboot_targets").strings())
        assertEquals(SettingsScope.entries.map { it.name }, fixture.getJSONArray("settings_scopes").strings())
        ENUM_LISTS.forEach { (key, names) -> fixture.optJSONArray(key)?.let { assertEquals(key, names, it.strings()) } }
        assertNull(Replies.caseResult(fixture.getJSONObject("ack_ok")))
        try {
            Replies.caseResult(fixture.getJSONObject("ack_err"))
            fail("an Err result must throw")
        } catch (expected: CaseErrorException) {
            assertEquals(CaseError.NotUnlocked, expected.error)
        }
        assertEquals(EqProfile.from(fixture.getJSONObject("eq_profile")), EqProfile.from(Replies.caseResult(fixture.getJSONObject("eq_result_ok")) as JSONObject))
        assertNull(Replies.caseResult(fixture.getJSONObject("eq_result_empty")))
        try {
            Replies.caseResult(fixture.getJSONObject("ack_err_unknown_code"))
            fail("an Err result must throw")
        } catch (expected: CaseErrorException) {
            assertEquals(CaseError.Unknown, expected.error)
            assertEquals("42", expected.message)
        }
        assertEquals(Capabilities.NAMES, fixture.getJSONArray("capability_names").strings())
        assertEquals(EventMask.NAMES, fixture.getJSONArray("event_names").strings())
        assertEquals(TopicMessage.PATHS, fixture.getJSONArray("topic_paths").strings())
        val stages = fixture.getJSONArray("stages").objects().associate { it.getInt("code") to it.getString("name") }
        assertEquals(stages, BootStages.NAMES)
    }

    @Test
    fun diagnosticsSubscriptionsAndTopicsParse() {
        val crashed = Diagnostics.from(Replies.caseResult(fixture.getJSONObject("diagnostics")) as JSONObject)
        assertTrue(crashed.safeMode)
        assertEquals("firmware safe mode", BootStages.name(crashed.stage))
        val crash = crashed.lastCrash!!
        assertEquals(CrashReason.Trap, crash.reason)
        assertEquals("firmware running", BootStages.name(crash.stage))
        assertEquals(0xFFFF_FFFFL, crash.mtval)
        assertEquals("runtime fault", crash.message)
        val clean = Diagnostics.from(Replies.caseResult(fixture.getJSONObject("diagnostics_clean")) as JSONObject)
        assertNull(clean.lastCrash)
        assertFalse(clean.safeMode)
        assertEquals("stage 0x123", BootStages.name(0x123))
        assertEquals(EventMask(keys = true, log = false, audio = false, uart = false), EventMask.from(Replies.caseResult(fixture.getJSONObject("events_result")) as JSONObject))
        val topics = fixture.getJSONArray("topic_messages").objects().map { TopicMessage.from(it) }
        assertEquals(KeyAction.Key(4), (topics[0] as TopicMessage.Key).event.action)
        assertEquals(LogLevel.Warn, (topics[1] as TopicMessage.Log).line.level)
        assertEquals(listOf(0x4B, 0x01), (topics[2] as TopicMessage.Uart).bytes)
    }

    @Test
    fun everyBridgeReplyParsesWithTheAppModels() {
        val replies = fixture.getJSONObject("bridge_replies")
        fun ok(name: String): Any? = Replies.parse(replies.getJSONObject(name).toString()).value
        fun err(name: String): BridgeFailure = try {
            Replies.parse(replies.getJSONObject(name).toString())
            fail("$name must be an error")
            throw AssertionError()
        } catch (expected: BridgeException) {
            expected.failure
        }
        val identity = ServiceIdentity.from(ok("identify") as JSONObject)
        assertTrue(identity.chipMatches)
        assertEquals("c86516", identity.jedec)
        assertEquals(InstallFlow.FLASH_BYTES, identity.capacity)
        val backup = BackupInfo.from(ok("backup") as JSONObject)
        assertEquals(InstallFlow.FLASH_BYTES, backup.size.toLong())
        assertEquals("Head", backup.stock)
        val pkg = PackageInfo.from(ok("open_package") as JSONObject)
        assertEquals(KumiVersion(0, 1, 0), pkg.version)
        assertEquals("Truncated", err("open_package_failed").kind)
        assertEquals("Kumi", (ok("install") as JSONObject).getString("boots"))
        val stopped = err("install_failed")
        assertEquals(0, stopped.resume)
        assertEquals("stock copy: erase", stopped.label)
        assertEquals("Stock", stopped.bootsAfter)
        assertNull(ok("reboot_from_service"))
        assertEquals(0, (ok("poll_topics") as JSONArray).length())
        val slot = SlotInfo.from(ok("update") as JSONObject)
        assertEquals(Slot.B, slot.slot)
        assertEquals(SlotState.Trial, slot.state)
        assertEquals("NoFreeSlot", err("update_refused").kind)
        assertNull(Replies.caseResult(ok("call_ok")))
        assertEquals("UnknownPath", err("call_failed").kind)
        val withTopics = Replies.parse(replies.getJSONObject("call_with_topics").toString())
        assertEquals(EventMask(keys = true, log = true, audio = false, uart = false), EventMask.from(Replies.caseResult(withTopics.value) as JSONObject))
        assertEquals("Event stream started", (withTopics.topics.single() as TopicMessage.Log).line.text)
        assertEquals(KumiClient.PROTOCOL, replies.getJSONObject("info").getInt("protocol"))
        assertEquals("InvalidProfile", err("eq_invalid_profile").kind)
        assertEquals("G2bWritePath", InstallFlow.gateOf(ok("install_gates_missing")))
        assertNull(InstallFlow.gateOf(ok("install_gates_complete")))
        assertNull("every gate the installer requires has recorded evidence", InstallFlow.gateOf(ok("install_gates_recorded")))
    }

    @Test
    fun developerRepliesParse() {
        val nack = I2cReply.from(Replies.caseResult(fixture.getJSONObject("i2c_nack")) as JSONObject)
        assertFalse(nack.acked)
        assertTrue(nack.data.isEmpty())
        val running = AudioBench.from(Replies.caseResult(fixture.getJSONObject("audio_bench_running")) as JSONObject)
        assertEquals(AudioBenchState.Running, running.state)
        assertEquals(144_000_000L, running.cpuHz)
        assertEquals(1_000L, running.calibration?.frameClockHz)
        assertEquals(2, running.calibration?.counts)
        assertEquals(AudioStart(48_000, AudioChannel.Left, -2_000, 1_000), running.stream?.start)
        assertEquals(1, running.stream?.dacs)
        assertNull(running.failure)
        val failed = AudioBench.from(Replies.caseResult(fixture.getJSONObject("audio_bench_failed")) as JSONObject)
        assertEquals(AudioBenchState.Failed, failed.state)
        assertNull(failed.stream)
        assertEquals(AudioPhase.Dac, failed.failure?.phase)
        assertTrue(failed.failure!!.fault.contains("Nack"))
        val future = JSONObject(fixture.getJSONObject("audio_bench_running").getJSONObject("Ok").toString()).put("state", "Cooling")
        assertEquals(AudioBenchState.Unknown, AudioBench.from(future).state)
        assertTrue(AudioStart(48_000, AudioChannel.Both, 0, 0).valid)
        assertFalse(AudioStart(48_000, AudioChannel.Both, -2_000, 24_000).valid)
        assertFalse(AudioStart(48_000, AudioChannel.Both, 100, 1_000).valid)
    }

    @Test
    fun everyEnumOrLimitListInTheFixtureIsCheckedByTheApp() {
        val lists = fixture.keys().asSequence().filter { key ->
            val value = fixture.get(key)
            value is JSONArray && value.length() > 0 && (0 until value.length()).all { value.get(it) is String }
        }.toSet()
        assertEquals(emptySet<String>(), lists - ENUM_LISTS.keys - CHECKED_LISTS)
        val limits = fixture.getJSONObject("limits")
        LIMITS.forEach { (key, value) ->
            assertTrue("the fixture no longer has the limit $key the app relies on", limits.has(key))
            assertEquals(key, value, limits.getLong(key))
        }
    }

    @Test
    fun unknownNamesFromANewerFirmwareDegradeInsteadOfCrashing() {
        val audio = JSONObject(fixture.getJSONObject("audio_status").toString())
        audio.put("jack", "LineOut")
        audio.getJSONObject("stream").put("kind", "Mqa")
        val status = AudioStatus.from(audio)
        assertEquals(Jack.Unknown, status.jack)
        assertEquals(StreamKind.Unknown, status.stream?.kind)

        val slots = JSONObject(fixture.getJSONObject("slots").toString()).put("booted", "C")
        slots.getJSONArray("slots").getJSONObject(1).put("state", "Quarantined")
        val table = SlotTable.from(slots)
        assertEquals(Slot.Unknown, table.booted)
        assertNull(table.running)
        assertEquals(SlotState.Unknown, table.slots[1].state)

        assertEquals(LogLevel.Unknown, LogLine.from(JSONObject(fixture.getJSONObject("log_line").toString()).put("level", "Fatal")).level)
        try {
            Replies.caseResult(JSONObject().put("Err", "Overheated"))
            fail("an Err result must throw")
        } catch (expected: CaseErrorException) {
            assertEquals(CaseError.Unknown, expected.error)
            assertEquals("Overheated", expected.message)
        }
        val macro = JSONObject().put("Macro", JSONArray().put(4).put(5))
        val action = KeyAction.from(macro)
        assertTrue(action is KeyAction.Unsupported)
        assertTrue(same(action.toJson(), macro))
        assertTrue(KeyAction.from("Sticky") is KeyAction.Unsupported)
        try {
            OutputSettings.from(JSONObject(fixture.getJSONObject("output").toString()).put("filter", "Minimum"))
            fail("a filter the app cannot send back must not parse")
        } catch (expected: BridgeException) {
            assertEquals("Malformed", expected.failure.kind)
        }
    }

    @Test
    fun slotsEventsAndLogsParse() {
        val slots = SlotTable.from(fixture.getJSONObject("slots"))
        assertEquals(Slot.A, slots.booted)
        assertEquals(SlotState.Confirmed, slots.running?.state)
        assertNull(slots.slots[1].version)
        val event = KeyEvent.from(fixture.getJSONObject("key_event"))
        assertEquals(KeyAction.Key(4), event.action)
        assertTrue(event.pressed)
        assertEquals(KeyboardStatus(true, 10, 1, 2, 0, true, 2), KeyboardStatus.from(fixture.getJSONObject("keyboard_status")))
        val log = LogLine.from(fixture.getJSONObject("log_line"))
        assertEquals(LogLevel.Warn, log.level)
        assertEquals("usb", log.module)
    }

    @Test
    fun theAppKnowsEveryEndpointTheBridgeRoutes() {
        val paths = fixture.getJSONArray("endpoints").strings().toSet()
        val used = setOf(
            "device/info", "audio/status", "audio/output/get", "audio/output/set", "audio/dsp/get", "audio/dsp/set",
            "audio/dac-extras/get", "audio/dac-extras/set", "audio/impedance", "audio/meters", "audio/test-tone",
            "audio/eq/get", "audio/eq/set", "audio/eq/select", "audio/mic/get", "audio/mic/set",
            "keys/map/get", "keys/map/set", "keys/map/clear", "keys/macro/get", "keys/macro/set",
            "keys/combo/get", "keys/combo/set", "keys/config/get", "keys/config/set", "keys/status", "settings/reset",
            "system/slots", "system/reboot", "system/confirm", "system/diagnostics", "system/telemetry",
            "dev/unlock", "dev/peek", "dev/poke", "dev/i2c", "dev/audio", "events/subscribe",
        )
        assertTrue((used - paths).toString(), paths.containsAll(used))
        val bridge = fixture.getJSONObject("bridge")
        assertEquals(CaseUsb.KUMI, UsbId(bridge.getInt("vendorId"), bridge.getInt("productId")))
        assertEquals(CaseUsb.SERVICE, UsbId(bridge.getInt("serviceVendorId"), bridge.getInt("serviceProductId")))
        assertEquals(listOf(KumiUsb.INTERFACE_CLASS, KumiUsb.INTERFACE_SUBCLASS, KumiUsb.INTERFACE_PROTOCOL), bridge.getJSONArray("interface").ints())
        assertEquals(ReleaseAsset.EXTENSION, bridge.getString("packageExtension"))
    }

    @Test
    fun repliesSeparateValuesErrorsAndTopics() {
        val reply = Replies.parse("""{"ok":{"Ok":null},"topics":[{"path":"keys/events","data":{"source":4,"pressed":false,"layer":1,"action":"Disabled"}}]}""")
        assertEquals(1, reply.topics.size)
        assertEquals(KeyAction.Disabled, (reply.topics[0] as TopicMessage.Key).event.action)
        try {
            Replies.parse("""{"err":{"kind":"VerifyFailed","step":12,"resume":10,"label":"slot A: program","boots":["Stock","Stock"]}}""")
            fail("an error reply must throw")
        } catch (expected: BridgeException) {
            assertEquals("VerifyFailed", expected.failure.kind)
            assertEquals(10, expected.failure.resume)
            assertEquals("Stock", expected.failure.bootsAfter)
        }
        assertFalse(Replies.parse("""{"ok":null}""").value != null)
    }

    private fun JSONArray.strings(): List<String> = (0 until length()).map(::getString)

    private object NoLink : KumiLink {
        override fun send(data: ByteArray) = true
        override fun receive(): ByteArray? = null
    }

    private inner class RecordingBridge : FakeBridge() {
        private val requests = mutableListOf<Pair<String, Any?>>()
        val paths: Set<String> get() = requests.map { it.first }.toSet()

        fun last(path: String): Any? = requests.last { it.first == path }.second

        override fun call(link: KumiLink, path: String, request: String): String {
            requests += path to JSONTokener(request).nextValue()
            return when (path) {
                "keys/map/get" -> """{"ok":{"Ok":{"entries":[],"more":false}}}"""
                "dev/peek" -> """{"ok":{"Ok":{"address":1056768,"words":[1,2,3,4]}}}"""
                "dev/poke" -> """{"ok":${fixture.getJSONObject("count_result")}}"""
                "dev/i2c" -> """{"ok":${fixture.getJSONObject("i2c_reply")}}"""
                "dev/audio" -> """{"ok":${fixture.getJSONObject("audio_bench_running")}}"""
                "events/subscribe" -> """{"ok":{"Ok":{"keys":true,"log":false,"audio":false,"uart":false}}}"""
                "audio/eq/get" -> """{"ok":{"Ok":null}}"""
                "keys/macro/get" -> """{"ok":{"Ok":null}}"""
                "keys/combo/get" -> """{"ok":{"Ok":null}}"""
                else -> """{"ok":{"Ok":null}}"""
            }
        }
    }

    companion object {
        private fun names(entries: List<Enum<*>>, vararg appOnly: Enum<*>): List<String> = (entries - appOnly.toSet()).map { it.name }

        val ENUM_LISTS: Map<String, List<String>> = mapOf(
            "case_errors" to names(CaseError.entries, CaseError.Unknown),
            "jacks" to names(Jack.entries),
            "stream_kinds" to names(StreamKind.entries, StreamKind.Unknown),
            "filter_kinds" to names(BandKind.entries),
            "slot_names" to names(Slot.entries, Slot.Unknown),
            "slot_states" to names(SlotState.entries, SlotState.Unknown),
            "log_levels" to names(LogLevel.entries, LogLevel.Unknown),
            "crash_reasons" to names(CrashReason.entries, CrashReason.Unknown),
            "audio_bench_states" to names(AudioBenchState.entries, AudioBenchState.Unknown),
            "audio_channels" to names(AudioChannel.entries),
            "audio_phases" to names(AudioPhase.entries, AudioPhase.Unknown),
            "class_h_modes" to names(ClassHMode.entries),
            "crossfeed_modes" to names(CrossfeedMode.entries),
            "case_hotkeys" to names(CaseHotkey.entries),
            "mouse_actions" to names(MouseAction.entries),
        )

        val CHECKED_LISTS = setOf("dac_filters", "reboot_targets", "settings_scopes", "endpoints", "capability_names", "event_names", "topic_paths")

        val LIMITS: Map<String, Long> = mapOf(
            "eq_slots" to EqProfile.SLOTS.toLong(),
            "eq_max_bands" to EqProfile.MAX_BANDS.toLong(),
            "eq_min_hz" to ParametricBand.MIN_HZ.toLong(),
            "eq_max_hz" to ParametricBand.MAX_HZ.toLong(),
            "eq_min_q_milli" to ParametricBand.MIN_Q_MILLI.toLong(),
            "eq_max_q_milli" to ParametricBand.MAX_Q_MILLI.toLong(),
            "eq_max_gain_centi_db" to ParametricBand.MAX_GAIN_CENTI_DB.toLong(),
            "eq_min_preamp_centi_db" to EqProfile.MIN_PREAMP_CENTI_DB.toLong(),
            "layers" to KeymapEntry.LAYERS.toLong(),
            "macro_slots" to KumiLimits.MACRO_SLOTS.toLong(),
            "macro_max_steps" to KumiLimits.MACRO_MAX_STEPS.toLong(),
            "combo_slots" to KumiLimits.COMBO_SLOTS.toLong(),
            "key_min_tapping_term_ms" to KumiLimits.KEY_MIN_TAPPING_TERM_MS.toLong(),
            "key_max_tapping_term_ms" to KumiLimits.KEY_MAX_TAPPING_TERM_MS.toLong(),
            "key_min_one_shot_ms" to KumiLimits.KEY_MIN_ONE_SHOT_MS.toLong(),
            "key_max_one_shot_ms" to KumiLimits.KEY_MAX_ONE_SHOT_MS.toLong(),
            "key_min_combo_term_ms" to KumiLimits.KEY_MIN_COMBO_TERM_MS.toLong(),
            "key_max_combo_term_ms" to KumiLimits.KEY_MAX_COMBO_TERM_MS.toLong(),
            "meter_floor_centi_db" to KumiLimits.METER_FLOOR_CENTI_DB.toLong(),
            "test_tone_min_hz" to KumiLimits.TEST_TONE_MIN_HZ.toLong(),
            "test_tone_max_hz" to KumiLimits.TEST_TONE_MAX_HZ.toLong(),
            "test_tone_min_ms" to KumiLimits.TEST_TONE_MIN_MS.toLong(),
            "test_tone_max_ms" to KumiLimits.TEST_TONE_MAX_MS.toLong(),
            "word_block" to KumiLimits.PEEK_MAX_WORDS.toLong(),
            "max_unlock_seconds" to KumiLimits.UNLOCK_MAX_SECONDS.toLong(),
            "mic_max_gain_centi_db" to KumiLimits.MIC_MAX_GAIN_CENTI_DB.toLong(),
            "output_max_balance_centi_db" to KumiLimits.OUTPUT_MAX_BALANCE_CENTI_DB.toLong(),
            "output_min_volume_limit_centi_db" to KumiLimits.OUTPUT_MIN_VOLUME_LIMIT_CENTI_DB.toLong(),
            "key_min_usage" to KumiLimits.KEY_MIN_USAGE.toLong(),
            "key_max_usage" to KumiLimits.KEY_MAX_USAGE.toLong(),
            "consumer_max_usage" to KumiLimits.CONSUMER_MAX_USAGE.toLong(),
            "eq_response_max_points" to KumiLimits.EQ_RESPONSE_MAX_POINTS.toLong(),
            "protocol" to KumiClient.PROTOCOL.toLong(),
            "byte_block" to KumiLimits.BYTE_BLOCK.toLong(),
            "i2c_max_address" to KumiLimits.I2C_MAX_ADDRESS.toLong(),
            "audio_min_level_centi_dbfs" to KumiLimits.AUDIO_MIN_LEVEL_CENTI_DBFS.toLong(),
            "audio_min_tone_hz" to KumiLimits.AUDIO_MIN_TONE_HZ.toLong(),
            "audio_max_tone_hz" to KumiLimits.AUDIO_MAX_TONE_HZ.toLong(),
        )
    }

    private fun same(a: Any?, b: Any?): Boolean = when {
        a is JSONObject && b is JSONObject -> a.keys().asSequence().toSet() == b.keys().asSequence().toSet() &&
            a.keys().asSequence().all { same(a.get(it), b.get(it)) }
        a is JSONArray && b is JSONArray -> a.length() == b.length() && (0 until a.length()).all { same(a.get(it), b.get(it)) }
        a is Number && b is Number -> a.toLong() == b.toLong() && a.toDouble() == b.toDouble()
        else -> a == b
    }
}
