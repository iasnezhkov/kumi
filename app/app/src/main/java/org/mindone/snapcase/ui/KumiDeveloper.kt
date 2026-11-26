// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.delay
import org.mindone.snapcase.R
import org.mindone.snapcase.kumi.AudioBench
import org.mindone.snapcase.kumi.AudioBenchRequest
import org.mindone.snapcase.kumi.AudioChannel
import org.mindone.snapcase.kumi.AudioStart
import org.mindone.snapcase.kumi.BootStages
import org.mindone.snapcase.kumi.CaseError
import org.mindone.snapcase.kumi.DevInput
import org.mindone.snapcase.kumi.DeveloperResult
import org.mindone.snapcase.kumi.KumiLimits
import org.mindone.snapcase.kumi.KumiSession

@Composable
fun KumiDeveloperScreen(kumi: KumiSession) {
    SuppressScreenKeys()
    val state by kumi.state.collectAsStateWithLifecycle()
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    val left = ((state.unlockedUntil - now) / 1000).coerceAtLeast(0)
    LaunchedEffect(state.unlockedUntil) {
        while (true) {
            now = System.currentTimeMillis()
            if (now >= state.unlockedUntil) break
            delay(1_000)
        }
    }
    val unlocked = left > 0
    val result = state.developer
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            SectionCard(stringResource(R.string.kumi_dev_title)) {
                Note(stringResource(R.string.kumi_dev_warning))
                if (unlocked) Note(stringResource(R.string.kumi_dev_left, left))
                Button(onClick = { kumi.unlockDeveloper(KumiLimits.UNLOCK_SECONDS) }) {
                    Text(stringResource(if (unlocked) R.string.kumi_dev_unlocked else R.string.kumi_dev_unlock))
                }
                if (state.ramOnly) Note(stringResource(R.string.kumi_dev_ram_only))
                state.error?.let { error ->
                    Note(error, MaterialTheme.colorScheme.error)
                    TextButton(onClick = kumi::clearError) { Text(stringResource(R.string.action_dismiss)) }
                }
            }
        }
        item { DiagnosticsCard(state.diagnostics, onRefresh = { kumi.refreshDiagnostics() }) }
        item { PeekCard(state.peek, unlocked) { address, words -> kumi.peek(address, words) } }
        item { PokeCard(result, unlocked) { address, value, mask -> kumi.poke(address, value, mask) } }
        item { I2cCard(result, unlocked) { bus, address, write, read -> kumi.i2c(bus, address, write, read) } }
        item { AudioBenchCard(result, unlocked) { kumi.audioBench(it) } }
        item {
            SectionCard(stringResource(R.string.kumi_dev_log)) {
                if (state.log.isEmpty()) Note(stringResource(R.string.kumi_dev_log_empty))
                state.log.takeLast(60).forEach { line -> Mono("%8.3f %-5s %s: %s".format(line.uptimeMs / 1000.0, line.level.name, line.module, line.text)) }
            }
        }
    }
}

@Composable
private fun DiagnosticsCard(diagnostics: org.mindone.snapcase.kumi.Diagnostics?, onRefresh: () -> Unit) {
    SectionCard(stringResource(R.string.kumi_dev_diagnostics)) {
        if (diagnostics == null) {
            Note(stringResource(R.string.kumi_dev_diagnostics_none))
        } else {
            Mono("boots ${diagnostics.bootCount} · stage ${BootStages.name(diagnostics.stage)} · up ${diagnostics.uptimeMs / 1000} s")
            Mono("safe mode ${diagnostics.safeMode} · watchdog ${diagnostics.watchdogArmed} · dropped events ${diagnostics.droppedEvents}")
            val crash = diagnostics.lastCrash
            if (crash == null) {
                Note(stringResource(R.string.kumi_dev_no_crash))
            } else {
                Text(stringResource(R.string.kumi_crash_title), style = MaterialTheme.typography.bodyLarge)
                Mono("${crash.reason.name} at ${BootStages.name(crash.stage)}, boot ${crash.bootCount}, ${crash.uptimeMs} ms")
                Mono("mcause %08x mepc %08x mtval %08x ra %08x sp %08x".format(crash.mcause, crash.mepc, crash.mtval, crash.ra, crash.sp))
                if (crash.message.isNotBlank()) Mono(crash.message)
            }
        }
        FilledTonalButton(onClick = onRefresh) { Text(stringResource(R.string.action_refresh)) }
    }
}

@Composable
private fun PeekCard(peek: org.mindone.snapcase.kumi.WordBlock?, unlocked: Boolean, onRead: (Long, Int) -> Unit) {
    var address by remember { mutableStateOf("00102000") }
    var words by remember { mutableStateOf("8") }
    SectionCard(stringResource(R.string.kumi_dev_peek)) {
        OutlinedTextField(address, { address = it.filter(Char::isLetterOrDigit).take(8) }, label = { Text(stringResource(R.string.kumi_dev_address)) })
        OutlinedTextField(words, { words = it.filter(Char::isDigit).take(2) }, label = { Text(stringResource(R.string.kumi_dev_words)) })
        val a = DevInput.address(address)
        val n = words.toIntOrNull()?.takeIf { it in 1..KumiLimits.PEEK_MAX_WORDS }
        FilledTonalButton(onClick = { if (a != null && n != null) onRead(a, n) }, enabled = unlocked && a != null && n != null) {
            Text(stringResource(R.string.action_read))
        }
        peek?.let { block ->
            block.words.chunked(4).forEachIndexed { row, chunk ->
                Mono("%08x: ".format(block.address + row * 16) + chunk.joinToString(" ") { "%08x".format(it) })
            }
        }
    }
}

@Composable
private fun PokeCard(result: DeveloperResult?, unlocked: Boolean, onWrite: (Long, Long, Long) -> Unit) {
    var address by remember { mutableStateOf("") }
    var value by remember { mutableStateOf("") }
    var mask by remember { mutableStateOf("ffffffff") }
    var confirming by remember { mutableStateOf(false) }
    val a = DevInput.address(address)
    val v = DevInput.word(value)
    val m = DevInput.word(mask)
    SectionCard(stringResource(R.string.kumi_dev_poke)) {
        Note(stringResource(R.string.kumi_dev_poke_text))
        OutlinedTextField(address, { address = it.filter(Char::isLetterOrDigit).take(8) }, label = { Text(stringResource(R.string.kumi_dev_address)) })
        OutlinedTextField(value, { value = it.filter(Char::isLetterOrDigit).take(8) }, label = { Text(stringResource(R.string.kumi_dev_value)) })
        OutlinedTextField(mask, { mask = it.filter(Char::isLetterOrDigit).take(8) }, label = { Text(stringResource(R.string.kumi_dev_mask)) })
        OutlinedButton(onClick = { confirming = true }, enabled = unlocked && a != null && v != null && m != null) { Text(stringResource(R.string.action_write)) }
        when (result) {
            is DeveloperResult.Poked -> Mono("%08x ← %s".format(result.address, DevInput.wordText(result.value)))
            is DeveloperResult.Refused -> if (result.action == "dev/poke") Note(refusal(result), MaterialTheme.colorScheme.error)
            else -> Unit
        }
    }
    if (confirming && a != null && v != null && m != null) {
        ConfirmDialog(
            title = stringResource(R.string.kumi_dev_poke),
            text = stringResource(R.string.kumi_dev_poke_confirm, DevInput.wordText(a), DevInput.wordText(v), DevInput.wordText(m)),
            onConfirm = { onWrite(a, v, m) },
            onDismiss = { confirming = false },
        )
    }
}

@Composable
private fun I2cCard(result: DeveloperResult?, unlocked: Boolean, onRun: (Int, Int, List<Int>, Int) -> Unit) {
    var bus by remember { mutableIntStateOf(0) }
    var address by remember { mutableStateOf("33") }
    var write by remember { mutableStateOf("") }
    var read by remember { mutableStateOf("0") }
    val a = DevInput.i2cAddress(address)
    val bytes = DevInput.bytes(write)
    val n = DevInput.readCount(read)
    SectionCard(stringResource(R.string.kumi_dev_i2c)) {
        Note(stringResource(R.string.kumi_dev_i2c_text))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            (0 until KumiLimits.I2C_BUSES).forEach { index ->
                FilterChip(selected = bus == index, onClick = { bus = index }, label = { Text(stringResource(R.string.kumi_dev_i2c_bus, index)) })
            }
        }
        OutlinedTextField(address, { address = it.filter(Char::isLetterOrDigit).take(2) }, label = { Text(stringResource(R.string.kumi_dev_i2c_address)) })
        OutlinedTextField(write, { write = it.take(3 * KumiLimits.BYTE_BLOCK) }, label = { Text(stringResource(R.string.kumi_dev_i2c_write)) })
        OutlinedTextField(read, { read = it.filter(Char::isDigit).take(2) }, label = { Text(stringResource(R.string.kumi_dev_i2c_read)) })
        FilledTonalButton(onClick = { if (a != null && bytes != null && n != null) onRun(bus, a, bytes, n) }, enabled = unlocked && a != null && bytes != null && n != null) {
            Text(stringResource(R.string.action_run))
        }
        when (result) {
            is DeveloperResult.I2c -> Mono(
                if (result.reply.acked) {
                    stringResource(R.string.kumi_dev_i2c_ack, result.bus, result.address, DevInput.hex(result.reply.data))
                } else {
                    stringResource(R.string.kumi_dev_i2c_nack, result.bus, result.address)
                },
            )
            is DeveloperResult.Refused -> if (result.action == "dev/i2c") Note(refusal(result), MaterialTheme.colorScheme.error)
            else -> Unit
        }
    }
}

@Composable
private fun AudioBenchCard(result: DeveloperResult?, unlocked: Boolean, onRequest: (AudioBenchRequest) -> Unit) {
    var rate by remember { mutableStateOf("48000") }
    var tone by remember { mutableStateOf("1000") }
    var level by remember { mutableStateOf("-30") }
    var channel by remember { mutableStateOf(AudioChannel.Both) }
    val start = run {
        val r = rate.toIntOrNull()
        val t = tone.toIntOrNull()
        val l = level.toDoubleOrNull()?.let { Math.round(it * 100).toInt() }
        if (r != null && t != null && l != null) AudioStart(r, channel, l, t).takeIf { it.valid } else null
    }
    SectionCard(stringResource(R.string.kumi_dev_bench)) {
        Note(stringResource(R.string.kumi_dev_bench_text))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            FilledTonalButton(onClick = { onRequest(AudioBenchRequest.Calibrate) }, enabled = unlocked) { Text(stringResource(R.string.kumi_dev_bench_calibrate)) }
            OutlinedButton(onClick = { onRequest(AudioBenchRequest.Status) }, enabled = unlocked) { Text(stringResource(R.string.kumi_dev_bench_status)) }
        }
        OutlinedTextField(rate, { rate = it.filter(Char::isDigit).take(6) }, label = { Text(stringResource(R.string.kumi_dev_bench_rate)) })
        OutlinedTextField(tone, { tone = it.filter(Char::isDigit).take(5) }, label = { Text(stringResource(R.string.kumi_dev_bench_tone)) })
        OutlinedTextField(level, { level = it.filter { c -> c.isDigit() || c == '-' || c == '.' }.take(6) }, label = { Text(stringResource(R.string.kumi_dev_bench_level)) })
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            AudioChannel.entries.forEach { option ->
                FilterChip(selected = channel == option, onClick = { channel = option }, label = { Text(stringResource(channelName(option))) })
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = { start?.let { onRequest(AudioBenchRequest.Start(it)) } }, enabled = unlocked && start != null) { Text(stringResource(R.string.kumi_dev_bench_start)) }
            OutlinedButton(onClick = { onRequest(AudioBenchRequest.Stop) }, enabled = unlocked) { Text(stringResource(R.string.kumi_dev_bench_stop)) }
        }
        when (result) {
            is DeveloperResult.Bench -> BenchReport(result.bench)
            is DeveloperResult.Refused -> if (result.action == "dev/audio") Note(refusal(result), MaterialTheme.colorScheme.error)
            else -> Unit
        }
    }
}

@Composable
private fun BenchReport(bench: AudioBench) {
    Mono("${bench.state.name} · CPU ${bench.cpuHz?.let { "$it Hz" } ?: "?"}")
    bench.calibration?.let { c ->
        Mono("frame clock ${c.frameClockHz ?: "?"} Hz · trims ${c.trims} · windows ${c.windows} · ${c.counts} counts")
        c.fault?.let { Mono("calibration fault $it") }
    }
    bench.stream?.let { s ->
        Mono("${s.start.rateHz} Hz ${s.start.channel.name} ${s.start.toneHz} Hz ${s.start.levelCentiDbfs / 100.0} dBFS · ${s.dacs} DAC")
        Mono("ring %08x, %d bytes, %d periods".format(s.ringAddress, s.ringBytes, s.periods))
    }
    bench.failure?.let { Mono("failed in ${it.phase.name}: ${it.fault}") }
}

private fun channelName(channel: AudioChannel): Int = when (channel) {
    AudioChannel.Left -> R.string.kumi_dev_bench_left
    AudioChannel.Right -> R.string.kumi_dev_bench_right
    AudioChannel.Both -> R.string.kumi_dev_bench_both
}

@Composable
private fun refusal(result: DeveloperResult.Refused): String = when (result.error) {
    CaseError.NotUnlocked -> stringResource(R.string.kumi_dev_refused_locked)
    CaseError.WrongState -> stringResource(if (result.action == "dev/audio") R.string.kumi_dev_refused_bench_state else R.string.kumi_dev_refused_ram)
    CaseError.Unsupported -> stringResource(R.string.kumi_dev_refused_unsupported)
    CaseError.InvalidArgument -> stringResource(R.string.kumi_dev_refused_argument)
    CaseError.Busy -> stringResource(R.string.kumi_dev_refused_busy)
    CaseError.HardwareFault -> stringResource(R.string.kumi_dev_refused_hardware)
    else -> result.raw
}
