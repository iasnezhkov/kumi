// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::thread::sleep;
use std::time::{Duration, Instant};

use kumi_audio::clock::Family;
use kumi_audio::dac::{self, Dac, PAIR};
use kumi_audio::flow::FRAME_BYTES;
use kumi_audio::i2c::{self, I2C0_BASE, I2cBus, I2cError};
use kumi_audio::i2s::{CHANNEL_BIT, Decoding, Direction, ProbeFault};
use kumi_audio::pcm::PcmProfile;
use kumi_audio::{Registers, board, clock, regs};
use kumi_client::ClientError;
use kumi_icd::{
    AUDIO_MONITOR_COMPLETIONS, AUDIO_MONITOR_CYCLES, AUDIO_MONITOR_DECODING, AUDIO_MONITOR_ERRORS, AUDIO_MONITOR_FEEDBACK,
    AUDIO_MONITOR_FRAME, AUDIO_MONITOR_LOST_PACKETS, AUDIO_MONITOR_OVERRUNS, AUDIO_MONITOR_PACKETS, AUDIO_MONITOR_PAUSES,
    AUDIO_MONITOR_POLLS, AUDIO_MONITOR_PROBE_RATE, AUDIO_MONITOR_QUEUED, AUDIO_MONITOR_UNDERRUNS, AUDIO_MONITOR_WORDS,
    AUDIO_MONITOR_WRAPS, AudioBench, AudioBenchRequest, AudioBenchState, AudioChannel, AudioStart, AudioStream, CaseError,
    ClassHMode, CrossfeedMode, DacExtras, DacFilter, DevAudio, DevI2c, DevPeek, DevPoke, DevUnlock, DevUnlockRequest,
    DspSettings, EQ_MAX_BANDS, EQ_SLOTS, EqBand, EqProfile, EqSlotUpdate, FilterKind, I2cRequest, OutputSettings, PeekRequest,
    PokeRequest, TestToneRequest,
};
use serde_json::{Value, json};

use super::{Connection, Failure, case_failure, hex_word, refused};

const UNLOCK_SECONDS: u16 = 300;
const SCAN_FIRST: u8 = 0x08;
const SCAN_LAST: u8 = 0x77;
const CONTROLLER_WORDS: u8 = 6;
const CLOCKS: [u32; 2] = [clock::SYSTEM_CLOCK, clock::CORE_PLL];
const VOLUMES: [(&str, u32); 2] = [("left", dac::VOLUME_LEFT), ("right", dac::VOLUME_RIGHT)];
const FAMILIES: [(&str, Family); 2] = [("48k", Family::Rate48k), ("44.1k", Family::Rate44k1)];
const TRIMS: usize = 16;
const NOMINAL_CPU_HZ: f64 = 60_000_000.0;
const EXPECTED_REFERENCE: u32 = 3_120;
const START_POLL: Duration = Duration::from_millis(20);
const START_TIMEOUT: Duration = Duration::from_secs(3);
const CURSOR_PERIOD: Duration = Duration::from_millis(10);
const DEFAULT_TONE_HZ: u32 = 1_000;
const DEFAULT_LEVEL_CENTI_DBFS: i16 = -2_000;
const MONITOR_NAMES: [(&str, usize); AUDIO_MONITOR_WORDS as usize] = [
    ("cycles", AUDIO_MONITOR_CYCLES),
    ("frame", AUDIO_MONITOR_FRAME),
    ("wraps", AUDIO_MONITOR_WRAPS),
    ("polls", AUDIO_MONITOR_POLLS),
    ("errors", AUDIO_MONITOR_ERRORS),
    ("decoding", AUDIO_MONITOR_DECODING),
    ("probe_counts_per_second", AUDIO_MONITOR_PROBE_RATE),
    ("completions", AUDIO_MONITOR_COMPLETIONS),
    ("queued_frames", AUDIO_MONITOR_QUEUED),
    ("feedback_q16", AUDIO_MONITOR_FEEDBACK),
    ("usb_packets", AUDIO_MONITOR_PACKETS),
    ("underruns", AUDIO_MONITOR_UNDERRUNS),
    ("overruns", AUDIO_MONITOR_OVERRUNS),
    ("lost_packets", AUDIO_MONITOR_LOST_PACKETS),
    ("host_pauses", AUDIO_MONITOR_PAUSES),
];
const COUNTERS: [(&str, usize); 6] = [
    ("usb_packets", AUDIO_MONITOR_PACKETS),
    ("underruns", AUDIO_MONITOR_UNDERRUNS),
    ("overruns", AUDIO_MONITOR_OVERRUNS),
    ("lost_packets", AUDIO_MONITOR_LOST_PACKETS),
    ("host_pauses", AUDIO_MONITOR_PAUSES),
    ("completions", AUDIO_MONITOR_COMPLETIONS),
];

struct Remote<'a> {
    client: &'a mut Connection,
    bus: u8,
    failure: Option<Failure>,
}

impl Remote<'_> {
    fn note(&mut self, failure: Failure) {
        if self.failure.is_none() {
            self.failure = Some(failure);
        }
    }

    fn poke(&mut self, address: u32, value: u32, mask: u32) {
        if let Err(error) = self.client.call_case::<DevPoke, _>(&PokeRequest { address, value, mask }) {
            self.note(case_failure("dev/poke", error));
        }
    }

    fn words(&mut self, address: u32, words: u8) -> Vec<String> {
        match self.client.call_case::<DevPeek, _>(&PeekRequest { address, words }) {
            Ok(block) => block.words.iter().map(|word| hex_word(*word)).collect(),
            Err(error) => {
                self.note(case_failure("dev/peek", error));
                Vec::new()
            }
        }
    }

    fn transaction(&mut self, address: u8, write: &[u8], read: &mut [u8]) -> Result<(), I2cError> {
        let (Ok(bytes), Ok(length)) = (heapless::Vec::from_slice(write), u8::try_from(read.len())) else {
            return Err(I2cError::Invalid);
        };
        let request = I2cRequest { bus: self.bus, address, write: bytes, read: length };
        match self.client.call_case::<DevI2c, _>(&request) {
            Ok(reply) if reply.acked && reply.data.len() == read.len() => {
                read.copy_from_slice(&reply.data);
                Ok(())
            }
            Ok(reply) if reply.acked => Err(I2cError::Bus),
            Ok(_) => Err(I2cError::Nack),
            Err(ClientError::Case(CaseError::InvalidArgument)) => Err(I2cError::Invalid),
            Err(ClientError::Case(CaseError::HardwareFault)) => Err(I2cError::Bus),
            Err(error) => {
                self.note(case_failure("dev/i2c", error));
                Err(I2cError::Unrecovered)
            }
        }
    }
}

impl Registers for Remote<'_> {
    fn read(&mut self, address: u32) -> u32 {
        match self.client.call_case::<DevPeek, _>(&PeekRequest { address, words: 1 }) {
            Ok(block) => block.words.first().copied().unwrap_or(0),
            Err(error) => {
                self.note(case_failure("dev/peek", error));
                0
            }
        }
    }

    fn write(&mut self, address: u32, value: u32) {
        self.poke(address, value, u32::MAX);
    }

    fn update(&mut self, address: u32, clear: u32, set: u32) {
        self.poke(address, set, clear | set);
    }

    fn delay_us(&mut self, microseconds: u32) {
        sleep(Duration::from_micros(u64::from(microseconds)));
    }
}

impl I2cBus for Remote<'_> {
    fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), I2cError> {
        self.transaction(address, bytes, &mut [])
    }

    fn write_then_read(&mut self, address: u8, prefix: &[u8], buffer: &mut [u8]) -> Result<(), I2cError> {
        self.transaction(address, prefix, buffer)
    }

    fn probe(&mut self, address: u8) -> Result<(), I2cError> {
        self.transaction(address, &[], &mut [])
    }
}

fn expectations(remote: &mut Remote) -> Vec<Value> {
    i2c::SETUP_EXPECT
        .iter()
        .map(|expect| {
            let actual = Registers::read(remote, expect.address) & expect.mask;
            json!({
                "address": hex_word(expect.address),
                "mask": hex_word(expect.mask),
                "expected": hex_word(expect.value),
                "actual": hex_word(actual),
                "ok": actual == expect.value,
            })
        })
        .collect()
}

fn dac_name(dac: Dac) -> String {
    format!("0x{:02x}", dac.address())
}

pub fn dac_probe(client: &mut Connection) -> Result<Value, Failure> {
    client.call_case::<DevUnlock, _>(&DevUnlockRequest { seconds: UNLOCK_SECONDS }).map_err(|e| case_failure("dev/unlock", e))?;
    let mut remote = Remote { client, bus: 0, failure: None };
    let clocks: Vec<Value> = CLOCKS
        .iter()
        .map(|&address| json!({"address": hex_word(address), "value": hex_word(Registers::read(&mut remote, address))}))
        .collect();
    let setup_before = expectations(&mut remote);
    regs::apply(&mut remote, &board::POWER_ON);
    regs::apply(&mut remote, &i2c::SETUP);
    let setup_after = expectations(&mut remote);
    regs::apply(&mut remote, &board::DAC_RESET_PULSE);
    let pair = dac::validate_pair(&mut remote);
    let negative = remote.probe(dac::NEGATIVE_CONTROL_ADDRESS);
    let controller_after_negative = remote.words(I2C0_BASE, CONTROLLER_WORDS);
    let mut volumes = serde_json::Map::new();
    let mut scan = Vec::new();
    match &pair {
        Ok(_) => {
            for dac in PAIR {
                for (side, register) in VOLUMES {
                    let value = match dac::read(&mut remote, dac, register) {
                        Ok(byte) => json!(format!("0x{byte:02x}")),
                        Err(error) => json!(format!("{error:?}")),
                    };
                    volumes.insert(format!("{}_{side}", dac_name(dac)), value);
                }
            }
        }
        Err(_) => {
            for address in SCAN_FIRST..=SCAN_LAST {
                if remote.probe(address).is_ok() {
                    scan.push(format!("0x{address:02x}"));
                }
            }
        }
    }
    regs::apply(&mut remote, &board::QUIET);
    regs::apply(&mut remote, &board::POWER_OFF);
    if let Some(failure) = remote.failure {
        return Err(failure);
    }
    let identities = match &pair {
        Ok(found) => json!(
            PAIR.iter()
                .zip(found)
                .map(|(dac, bytes)| json!({
                    "address": dac_name(*dac),
                    "identity": bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                }))
                .collect::<Vec<_>>()
        ),
        Err(error) => json!({"error": format!("{error:?}")}),
    };
    Ok(json!({
        "clocks": clocks,
        "setup_before": setup_before,
        "setup_after": setup_after,
        "dac_pair": identities,
        "negative_control_0x32": format!("{negative:?}"),
        "i2c0_after_negative_control": controller_after_negative,
        "volumes": volumes,
        "bus0_scan_acked": scan,
        "left_quiet": true,
    }))
}

fn usage(message: &str) -> Failure {
    Failure { code: 2, message: message.into() }
}

fn unlock(client: &mut Connection) -> Result<(), Failure> {
    client.call_case::<DevUnlock, _>(&DevUnlockRequest { seconds: UNLOCK_SECONDS }).map_err(|e| case_failure("dev/unlock", e))
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn bench(client: &mut Connection, request: AudioBenchRequest) -> Result<AudioBench, Failure> {
    match client.call_case::<DevAudio, _>(&request) {
        Ok(bench) => Ok(bench),
        Err(ClientError::Case(CaseError::HardwareFault)) => {
            let status = client.call_case::<DevAudio, _>(&AudioBenchRequest::Status).map(|bench| describe(&bench));
            Err(Failure::from(format!(
                "dev/audio: the hardware step failed; the bench reports\n{}",
                pretty(&status.unwrap_or_else(|error| json!(format!("{error:?}"))))
            )))
        }
        Err(ClientError::Case(CaseError::Busy)) => {
            Err(Failure::from("dev/audio: a stream is starting or running; run `kumi audio stop` first"))
        }
        Err(ClientError::Case(CaseError::WrongState)) => Err(refused(
            "dev/audio: no calibration yet (run `kumi audio calibrate` first), or the runtime was not loaded into RAM by a host"
                .into(),
        )),
        Err(ClientError::Case(CaseError::InvalidArgument)) => Err(usage(
            "dev/audio: the runtime refused the arguments: the rate is not a PCM rate of kumi-audio, or no whole number of tone \
             periods fits its DMA ring (choose a tone that divides the rate more evenly, such as 1000 Hz)",
        )),
        Err(error) => Err(case_failure("dev/audio", error)),
    }
}

fn hex_words(words: &[u32]) -> Vec<String> {
    words.iter().map(|word| hex_word(*word)).collect()
}

fn ring_frames(stream: &AudioStream) -> u32 {
    stream.ring_bytes / FRAME_BYTES as u32
}

fn describe_stream(stream: &AudioStream) -> Value {
    let frames = ring_frames(stream);
    let dacs: Vec<&str> = [(1, "0x33"), (2, "0x31")].iter().filter(|(bit, _)| stream.dacs & bit != 0).map(|(_, a)| *a).collect();
    let rate = f64::from(stream.start.rate_hz);
    json!({
        "rate_hz": stream.start.rate_hz,
        "channel": stream.start.channel,
        "tone_hz": stream.start.tone_hz,
        "level_dbfs": f64::from(stream.start.level_centi_dbfs) / 100.0,
        "amplitude": stream.amplitude,
        "dacs": dacs,
        "ring": {
            "address": hex_word(stream.ring_address),
            "bytes": stream.ring_bytes,
            "frames": frames,
            "tone_periods": stream.periods,
            "period_ms": f64::from(frames) * 1_000.0 / rate,
            "expected_wraps_per_second": rate / f64::from(frames.max(1)),
        },
        "started_ms": stream.started_us.map(|us| f64::from(us) / 1_000.0),
        "peek": {
            "dma_remaining": hex_word(stream.cursor.remaining),
            "dma_status": hex_word(stream.cursor.status),
            "dma_enable": hex_word(stream.cursor.enable),
            "i2s_status": hex_word(stream.cursor.i2s_status),
            "monitor": hex_word(stream.cursor.monitor),
            "monitor_words": MONITOR_NAMES.map(|(name, _)| name),
        },
    })
}

fn describe_calibration(bench: &AudioBench) -> Value {
    let Some(report) = &bench.calibration else { return Value::Null };
    let families: Vec<Value> = FAMILIES
        .iter()
        .enumerate()
        .map(|(index, (name, family))| {
            let profile = family.pll();
            let sweep: Vec<Value> = report
                .counts
                .iter()
                .skip(index * TRIMS)
                .take(TRIMS)
                .enumerate()
                .map(|(trim, [reference, measured])| {
                    json!({
                        "trim": trim,
                        "reference": reference,
                        "measured": measured,
                        "bounds": profile.bounds(*reference),
                        "accepted": profile.accepts(*reference, *measured),
                    })
                })
                .collect();
            json!({
                "family": name,
                "master_hz": family.master_hz(),
                "trim": report.trims[index],
                "window": format!("0x{:04x}", report.windows[index]),
                "sweep": sweep,
            })
        })
        .collect();
    let agree = match (report.decoded_cpu_hz, report.frame_clock_hz) {
        (Some(decoded), Some(measured)) => json!(f64::from(decoded.abs_diff(measured)) <= f64::from(decoded) * 1e-3),
        _ => Value::Null,
    };
    json!({
        "system_clock": hex_word(report.system_clock),
        "core_pll": hex_word(report.core_pll),
        "cpu_hz": {"decoded": report.decoded_cpu_hz, "frame_clock": report.frame_clock_hz, "agree_within_0_1_percent": agree},
        "timers_quiesced": report.timers_quiesced,
        "fault": report.fault,
        "families": families,
        "counts_read": report.counts.len(),
        "expected_reference": EXPECTED_REFERENCE,
        "cycles": report.cycles,
        "milliseconds": f64::from(report.cycles) * 1_000.0 / NOMINAL_CPU_HZ,
        "pll_after": {
            "control": hex_word(report.pll[0]),
            "fraction": hex_word(report.pll[1]),
            "trim": hex_word(report.pll[2]),
        },
    })
}

fn describe(bench: &AudioBench) -> Value {
    json!({
        "state": bench.state,
        "frame_clock_cpu_hz": bench.cpu_hz,
        "calibration": describe_calibration(bench),
        "stream": bench.stream.as_ref().map(describe_stream),
        "failure": bench.failure,
        "halt": bench.halt.map(|halt| json!({
            "cycles": halt.cycles,
            "milliseconds": f64::from(halt.cycles) * 1_000.0 / NOMINAL_CPU_HZ,
            "dma_enable": hex_word(halt.enable),
            "channel_enabled": halt.enable & CHANNEL_BIT != 0,
            "remaining": hex_words(&halt.remaining),
            "remaining_still": halt.remaining[0] == halt.remaining[1],
        })),
    })
}

pub fn calibrate(client: &mut Connection) -> Result<Value, Failure> {
    unlock(client)?;
    Ok(describe(&bench(client, AudioBenchRequest::Calibrate)?))
}

pub fn status(client: &mut Connection) -> Result<Value, Failure> {
    unlock(client)?;
    Ok(describe(&bench(client, AudioBenchRequest::Status)?))
}

pub fn stop(client: &mut Connection) -> Result<Value, Failure> {
    unlock(client)?;
    Ok(describe(&bench(client, AudioBenchRequest::Stop)?))
}

fn level(text: &str) -> Result<i16, Failure> {
    let decibels = text.parse::<f64>().ok().filter(|value| value.is_finite() && *value <= 0.0);
    let centi = decibels.map(|value| (value * 100.0).round()).filter(|value| *value >= f64::from(i16::MIN));
    centi.map(|value| value as i16).ok_or_else(|| usage("give the level in dBFS as 0 or a negative number, for example -20"))
}

pub fn parse_start(words: &[String], tone: bool) -> Result<AudioStart, Failure> {
    let shape = if tone { "audio tone RATE [left|right|both] [TONE_HZ] [LEVEL_DB]" } else { "audio silence RATE" };
    if words.is_empty() || words.len() > if tone { 4 } else { 1 } {
        return Err(usage(shape));
    }
    let rate_hz = words[0].parse::<u32>().map_err(|_| usage(shape))?;
    if PcmProfile::from_hz(rate_hz).is_none() {
        return Err(usage("RATE must be one of 44100, 48000, 88200, 96000, 176400, 192000, 352800, 384000"));
    }
    let channel = match words.get(1).map(String::as_str) {
        None | Some("both") => AudioChannel::Both,
        Some("left") => AudioChannel::Left,
        Some("right") => AudioChannel::Right,
        Some(_) => return Err(usage(shape)),
    };
    let tone_hz = match words.get(2) {
        Some(text) => text.parse::<u32>().map_err(|_| usage(shape))?,
        None if tone => DEFAULT_TONE_HZ,
        None => 0,
    };
    let level_centi_dbfs = match words.get(3) {
        Some(text) => level(text)?,
        None if tone => DEFAULT_LEVEL_CENTI_DBFS,
        None => 0,
    };
    let start = AudioStart { rate_hz, channel, level_centi_dbfs, tone_hz };
    if !start.is_valid() || (tone && tone_hz == 0) {
        return Err(usage(&format!(
            "the tone must be {}..{} Hz and below half the rate, the level {}..0 dBFS",
            AudioStart::MIN_TONE_HZ,
            AudioStart::MAX_TONE_HZ,
            AudioStart::MIN_LEVEL_CENTI_DBFS / 100
        )));
    }
    Ok(start)
}

pub fn start(client: &mut Connection, request: AudioStart) -> Result<Value, Failure> {
    unlock(client)?;
    let began = Instant::now();
    let mut current = bench(client, AudioBenchRequest::Start(request))?;
    let mut polls = 0u32;
    while current.state == AudioBenchState::Starting && began.elapsed() < START_TIMEOUT {
        sleep(START_POLL);
        current = bench(client, AudioBenchRequest::Status)?;
        polls += 1;
    }
    let mut value = describe(&current);
    value["status_polls"] = json!(polls);
    value["host_ms"] = json!(began.elapsed().as_millis() as u64);
    match current.state {
        AudioBenchState::Running => {
            value["next"] = json!("`kumi ... audio cursor 1` samples the DMA cursor; `kumi ... audio stop` ends the stream");
            Ok(value)
        }
        AudioBenchState::Starting => Err(Failure::from(format!(
            "dev/audio: the stream did not start within {} s; run `kumi audio stop`\n{}",
            START_TIMEOUT.as_secs(),
            pretty(&value)
        ))),
        AudioBenchState::Failed | AudioBenchState::Idle => {
            Err(Failure::from(format!("dev/audio: the stream did not start\n{}", pretty(&value))))
        }
    }
}

struct Sample {
    host_us: f64,
    monitor: Vec<u32>,
    remaining: u32,
}

fn peek_words(client: &mut Connection, address: u32, words: u8) -> Result<Vec<u32>, Failure> {
    let block = client.call_case::<DevPeek, _>(&PeekRequest { address, words }).map_err(|e| case_failure("dev/peek", e))?;
    if block.words.len() != usize::from(words) {
        return Err(Failure::from(format!("dev/peek: {} words instead of {words}", block.words.len())));
    }
    Ok(block.words.to_vec())
}

fn decoding(monitor: &[u32]) -> Decoding {
    Decoding::from_word(monitor[AUDIO_MONITOR_DECODING]).unwrap_or(Decoding::REMAINING_BYTES)
}

fn register_frame(ring_bytes: u32, decoding: Decoding, remaining: u32) -> Option<u32> {
    decoding.offset(remaining, ring_bytes).map(|offset| offset / FRAME_BYTES as u32)
}

fn describe_decoding(monitor: &[u32]) -> Value {
    let word = monitor[AUDIO_MONITOR_DECODING];
    let probe_rate = monitor[AUDIO_MONITOR_PROBE_RATE];
    match (Decoding::from_word(word), ProbeFault::from_code(word >> 24)) {
        (Some(decoding), _) => json!({
            "learned": true,
            "direction": match decoding.direction {
                Direction::Down => "down",
                Direction::Up => "up",
            },
            "unit_bytes": decoding.unit_bytes,
            "bias": decoding.bias,
            "probe_counts_per_second": probe_rate,
        }),
        (None, Some(fault)) => json!({
            "learned": false,
            "probe_fault": format!("{fault:?}"),
            "probe_counts_per_second": probe_rate,
            "assumed": "remaining bytes minus one, counting down",
        }),
        (None, None) => json!({"learned": false, "assumed": "remaining bytes minus one, counting down"}),
    }
}

pub fn cursor(client: &mut Connection, seconds: u64) -> Result<Value, Failure> {
    unlock(client)?;
    let current = bench(client, AudioBenchRequest::Status)?;
    let Some(stream) = current.stream.filter(|_| current.state == AudioBenchState::Running) else {
        return Err(Failure::from("dev/audio: no stream is running; start one with `kumi audio silence RATE` or `audio tone`"));
    };
    let frames = ring_frames(&stream);
    let map = stream.cursor;
    let errors_before = [peek_words(client, map.i2s_status, 1)?[0], peek_words(client, map.status, 1)?[0]];
    let began = Instant::now();
    let mut samples = Vec::new();
    let mut tick = 0u32;
    while began.elapsed() < Duration::from_secs(seconds) {
        let before = began.elapsed();
        let monitor = peek_words(client, map.monitor, AUDIO_MONITOR_WORDS)?;
        let remaining = peek_words(client, map.remaining, 1)?[0];
        let after = began.elapsed();
        let sample = Sample { host_us: (before + after).as_secs_f64() * 500_000.0, monitor, remaining };
        println!(
            "{}",
            json!({
                "t_ms": sample.host_us / 1_000.0,
                "frame": sample.monitor[AUDIO_MONITOR_FRAME],
                "wraps": sample.monitor[AUDIO_MONITOR_WRAPS],
                "register_frame": register_frame(stream.ring_bytes, decoding(&sample.monitor), sample.remaining),
                "queued_frames": sample.monitor[AUDIO_MONITOR_QUEUED],
                "feedback_q16": hex_word(sample.monitor[AUDIO_MONITOR_FEEDBACK]),
                "underruns": sample.monitor[AUDIO_MONITOR_UNDERRUNS],
                "overruns": sample.monitor[AUDIO_MONITOR_OVERRUNS],
                "lost_packets": sample.monitor[AUDIO_MONITOR_LOST_PACKETS],
                "host_pauses": sample.monitor[AUDIO_MONITOR_PAUSES],
            })
        );
        samples.push(sample);
        tick += 1;
        if let Some(wait) = (CURSOR_PERIOD * tick).checked_sub(began.elapsed()) {
            sleep(wait);
        }
    }
    let errors_after = [peek_words(client, map.i2s_status, 1)?[0], peek_words(client, map.status, 1)?[0]];
    let sysclk = peek_words(client, clock::SYSTEM_CLOCK, 1)?[0];
    let core_pll = peek_words(client, clock::CORE_PLL, 1)?[0];
    let cpu_hz = clock::decode_cpu_hz(sysclk, core_pll).unwrap_or(NOMINAL_CPU_HZ as u32);
    let mut summary = analyse(&samples, frames, stream.start.rate_hz, cpu_hz);
    summary["rate_hz"] = json!(stream.start.rate_hz);
    summary["cpu_hz"] = json!(cpu_hz);
    summary["ring_frames"] = json!(frames);
    summary["i2s_status_before_after"] = json!(hex_words(&[errors_before[0], errors_after[0]]));
    summary["dma_status_before_after"] = json!(hex_words(&[errors_before[1], errors_after[1]]));
    Ok(summary)
}

pub fn telemetry(client: &mut Connection) -> Result<Value, Failure> {
    unlock(client)?;
    let sysclk = peek_words(client, clock::SYSTEM_CLOCK, 1)?[0];
    let pll_core = peek_words(client, clock::CORE_PLL, 1)?[0];
    let cpu_hz = clock::decode_cpu_hz(sysclk, pll_core);
    let pmu_pwr = peek_words(client, 0x2100_4000, 1)?[0];
    let pmu_vol = peek_words(client, 0x2100_4004, 1)?[0];
    let dvdd_code = (pmu_vol >> 8) & 7;
    let audio_pll = peek_words(client, clock::AUDIO_PLL, 1)?[0];
    let rail_cmu = peek_words(client, 0x2000_20f8, 1)?[0];
    let rail_300 = peek_words(client, 0x2011_0300, 1)?[0];
    let rail_310 = peek_words(client, 0x2011_0310, 1)?[0];
    let gpio5 = peek_words(client, board::OUTPUT_ENABLE, 1)?[0];
    let gpio34 = peek_words(client, board::DAC_RESET, 1)?[0];
    let gpio_in0 = peek_words(client, 0x2011_0110, 1)?[0];
    let lradc0 = peek_words(client, 0x2101_0050, 1)?[0] & 0x3ff;
    let lradc5 = peek_words(client, 0x2101_0064, 1)?[0] & 0x3ff;
    let lradc12 = peek_words(client, 0x2101_0080, 1)?[0] & 0x3ff;
    let cmu_clken0 = peek_words(client, 0x2000_2000, 1)?[0];
    let rmu_rsten0 = peek_words(client, 0x2000_3000, 1)?[0];
    let dev_wfi = peek_words(client, 0x2000_5000, 1)?[0];
    let plic_enable = peek_words(client, 0x1000_2000, 2)?;
    let plic_pending = peek_words(client, 0x1000_1000, 2)?;
    let timer_active = (cmu_clken0 & (1 << 5) != 0) && (rmu_rsten0 & (1 << 5) != 0);
    let (timer_int_en, timer3_ctl) =
        if timer_active { (peek_words(client, 0x2001_0004, 1)?[0], peek_words(client, 0x2001_0080, 1)?[0]) } else { (0, 0) };
    let otp = peek_words(client, 0x2000_0010, 8)?;
    Ok(json!({
        "cpu_hz": cpu_hz,
        "sysclk": hex_word(sysclk),
        "pll_core": hex_word(pll_core),
        "pmu_pwr": hex_word(pmu_pwr),
        "pmu_vol": hex_word(pmu_vol),
        "external_dcdc": pmu_pwr & (1 << 25) != 0,
        "internal_ldo": pmu_pwr & (1 << 10) != 0,
        "lradc_bias": pmu_pwr & (1 << 12) != 0,
        "dvdd_vol_code": dvdd_code,
        "dvdd_mv": 800 + dvdd_code * 100,
        "audio_pll": hex_word(audio_pll),
        "audio_pll_enabled": audio_pll & clock::PLL_ENABLE != 0,
        "analog_rails": {
            "cmu_20f8": hex_word(rail_cmu),
            "gpio_300": hex_word(rail_300),
            "gpio_310": hex_word(rail_310),
            "on": (rail_cmu | rail_300 | rail_310) != 0,
        },
        "gpio5_awake": gpio5 & board::PIN_HIGH == board::PIN_HIGH,
        "gpio5": hex_word(gpio5),
        "gpio34_dac_reset": hex_word(gpio34),
        "gpio17_high": (gpio_in0 >> 17) & 1 != 0,
        "lradc": {
            "jack_ch0": lradc0,
            "avdd_ch5": lradc5,
            "internal_ch12": lradc12,
        },
        "wfi": {
            "dev_wfi": hex_word(dev_wfi),
            "plic_enable": hex_words(&plic_enable),
            "plic_pending": hex_words(&plic_pending),
            "timer3_running": timer_active && (timer3_ctl & 1 != 0),
            "timer3_int_en": timer_active && (timer_int_en & 8 != 0),
        },
        "efuse_otp": hex_words(&otp),
    }))
}

pub fn impedance(client: &mut Connection) -> Result<Value, Failure> {
    let report = client.measure_impedance().map_err(|e| case_failure("audio/impedance", e))?;
    Ok(json!({
        "left_ohms": report.left_ohms,
        "right_ohms": report.right_ohms,
        "left_ac_ohms": report.left_ac_ohms,
        "right_ac_ohms": report.right_ac_ohms,
        "left_status": format!("0x{:02x}", report.left_status),
        "right_status": format!("0x{:02x}", report.right_status),
        "hp_status": format!("0x{:02x}", report.hp_status),
        "load_stat": format!("0x{:02x}", report.load_stat),
    }))
}

pub fn test_tone(client: &mut Connection, args: &[String]) -> Result<Value, Failure> {
    let shape = "audio test-tone [left|right|both|phase|stop] [HZ] [MS]";
    let (channel, invert_right, frequency_hz, duration_ms) = match args.first().map(String::as_str) {
        Some("stop") => (AudioChannel::Both, false, 0, 0),
        other => {
            let (ch, inv) = match other {
                None | Some("both") => (AudioChannel::Both, false),
                Some("left") => (AudioChannel::Left, false),
                Some("right") => (AudioChannel::Right, false),
                Some("phase") => (AudioChannel::Both, true),
                _ => return Err(usage(shape)),
            };
            let hz = match args.get(1) {
                Some(raw) => raw.parse::<u16>().map_err(|_| usage(shape))?,
                None => 1_000,
            };
            let ms = match args.get(2) {
                Some(raw) => raw.parse::<u16>().map_err(|_| usage(shape))?,
                None => 1_200,
            };
            (ch, inv, hz, ms)
        }
    };
    let req = TestToneRequest { channel, frequency_hz, duration_ms, invert_right };
    if !req.is_valid() {
        return Err(usage(shape));
    }
    client.play_test_tone(req).map_err(|e| case_failure("audio/test-tone", e))?;
    Ok(json!(req))
}

fn rate(frames: f64, seconds: f64, nominal: f64) -> Value {
    if seconds <= 0.0 {
        return Value::Null;
    }
    let per_second = frames / seconds;
    json!({"frames_per_second": per_second, "ppm": (per_second / nominal - 1.0) * 1e6})
}

fn analyse(samples: &[Sample], ring_frames: u32, rate_hz: u32, cpu_hz: u32) -> Value {
    let (Some(first), Some(last)) = (samples.first(), samples.last()) else { return json!({"samples": 0}) };
    let ring = u64::from(ring_frames);
    let position =
        |sample: &Sample| u64::from(sample.monitor[AUDIO_MONITOR_WRAPS]) * ring + u64::from(sample.monitor[AUDIO_MONITOR_FRAME]);
    let decoding = decoding(&last.monitor);
    let mut cycles = 0u64;
    let mut register_frames = 0u64;
    let mut register_wraps = 0u32;
    let mut torn = 0u32;
    let mut gap_us = 0f64;
    for pair in samples.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        cycles += u64::from(b.monitor[AUDIO_MONITOR_CYCLES].wrapping_sub(a.monitor[AUDIO_MONITOR_CYCLES]));
        gap_us = gap_us.max(b.host_us - a.host_us);
        match (
            register_frame(ring_frames * FRAME_BYTES as u32, decoding, a.remaining),
            register_frame(ring_frames * FRAME_BYTES as u32, decoding, b.remaining),
        ) {
            (Some(x), Some(y)) => {
                register_frames += u64::from((y + ring_frames - x) % ring_frames.max(1));
                register_wraps += u32::from(y < x);
            }
            _ => torn += 1,
        }
    }
    let frames = position(last).wrapping_sub(position(first)) as f64;
    let host_seconds = (last.host_us - first.host_us) / 1e6;
    let nominal = f64::from(rate_hz);
    let period_ms = f64::from(ring_frames) * 1_000.0 / nominal;
    let counters: serde_json::Map<String, Value> = COUNTERS
        .iter()
        .map(|(name, index)| ((*name).to_string(), json!(last.monitor[*index].wrapping_sub(first.monitor[*index]))))
        .collect();
    let queued = samples.iter().map(|sample| sample.monitor[AUDIO_MONITOR_QUEUED]);
    json!({
        "samples": samples.len(),
        "host_seconds": host_seconds,
        "mean_interval_ms": if samples.len() > 1 { host_seconds * 1_000.0 / (samples.len() - 1) as f64 } else { 0.0 },
        "max_interval_ms": gap_us / 1_000.0,
        "ring_period_ms": period_ms,
        "firmware": {
            "frames": frames,
            "wraps": last.monitor[AUDIO_MONITOR_WRAPS].wrapping_sub(first.monitor[AUDIO_MONITOR_WRAPS]),
            "expected_wraps": nominal * host_seconds / f64::from(ring_frames.max(1)),
            "by_host_clock": rate(frames, host_seconds, nominal),
            "by_mcycle": rate(frames, cycles as f64 / f64::from(cpu_hz.max(1)), nominal),
            "cursor_polls": last.monitor[AUDIO_MONITOR_POLLS].wrapping_sub(first.monitor[AUDIO_MONITOR_POLLS]),
            "cursor_errors": last.monitor[AUDIO_MONITOR_ERRORS],
        },
        "decoding": describe_decoding(&last.monitor),
        "counters": counters,
        "queued_frames": {"min": queued.clone().min(), "max": queued.max(), "last": last.monitor[AUDIO_MONITOR_QUEUED]},
        "feedback_q16": hex_word(last.monitor[AUDIO_MONITOR_FEEDBACK]),
        "registers": {
            "frames": register_frames,
            "wraps": register_wraps,
            "unreadable_samples": torn,
            "by_host_clock": rate(register_frames as f64, host_seconds, nominal),
            "wraps_trustworthy": gap_us / 1_000.0 < period_ms,
        },
    })
}

fn parse_centi_db(text: &str, min: i16, max: i16, what: &str) -> Result<i16, Failure> {
    let db = text.parse::<f64>().ok().filter(|value| value.is_finite());
    let centi = db.map(|value| (value * 100.0).round()).filter(|value| *value >= f64::from(min) && *value <= f64::from(max));
    centi
        .map(|value| value as i16)
        .ok_or_else(|| usage(&format!("{what} must be in {}..{} dB", f64::from(min) / 100.0, f64::from(max) / 100.0)))
}

fn parse_filter(text: &str) -> Result<DacFilter, Failure> {
    match text {
        "fast-ll" => Ok(DacFilter::FastLowLatency),
        "fast-pc" => Ok(DacFilter::FastPhaseCompensated),
        "slow-ll" => Ok(DacFilter::SlowLowLatency),
        "slow-pc" => Ok(DacFilter::SlowPhaseCompensated),
        "nos" => Ok(DacFilter::NonOversampling),
        _ => Err(usage("filter must be fast-ll, fast-pc, slow-ll, slow-pc or nos")),
    }
}

pub fn parse_output_update(current: OutputSettings, args: &[String]) -> Result<OutputSettings, Failure> {
    let shape = "output set [--filter fast-ll|fast-pc|slow-ll|slow-pc|nos] [--limit DB] [--balance DB] [--dsd-direct on|off]";
    if args.is_empty() {
        return Err(usage(shape));
    }
    let mut next = current;
    let mut i = 0;
    while i < args.len() {
        let Some(value) = args.get(i + 1) else { return Err(usage(shape)) };
        match args[i].as_str() {
            "--filter" => next.filter = parse_filter(value)?,
            "--limit" => {
                next.volume_limit_centi_db = parse_centi_db(value, OutputSettings::MIN_VOLUME_LIMIT_CENTI_DB, 0, "limit")?
            }
            "--balance" => {
                next.balance_centi_db = parse_centi_db(
                    value,
                    -OutputSettings::MAX_BALANCE_CENTI_DB,
                    OutputSettings::MAX_BALANCE_CENTI_DB,
                    "balance",
                )?;
            }
            "--dsd-direct" => match value.as_str() {
                "on" | "true" | "1" => next.dsd_direct = true,
                "off" | "false" | "0" => next.dsd_direct = false,
                _ => return Err(usage("--dsd-direct must be on or off")),
            },
            _ => return Err(usage(shape)),
        }
        i += 2;
    }
    Ok(next)
}

pub fn output(client: &mut Connection, args: &[String]) -> Result<Value, Failure> {
    match args.first().map(String::as_str) {
        None => Ok(json!(client.output().map_err(|e| case_failure("audio/output/get", e))?)),
        Some("set") => {
            let current = client.output().map_err(|e| case_failure("audio/output/get", e))?;
            let next = parse_output_update(current, &args[1..])?;
            client.set_output(next).map_err(|e| case_failure("audio/output/set", e))?;
            Ok(json!(next))
        }
        _ => Err(usage("output [set [--filter ...] [--limit DB] [--balance DB] [--dsd-direct on|off]]")),
    }
}

fn parse_bool(text: &str, flag: &str) -> Result<bool, Failure> {
    match text {
        "on" | "true" | "1" => Ok(true),
        "off" | "false" | "0" => Ok(false),
        _ => Err(usage(&format!("{flag} must be on or off"))),
    }
}

fn parse_crossfeed(text: &str) -> Result<CrossfeedMode, Failure> {
    match text {
        "off" => Ok(CrossfeedMode::Off),
        "subtle" => Ok(CrossfeedMode::Subtle),
        "natural" => Ok(CrossfeedMode::Natural),
        "strong" => Ok(CrossfeedMode::Strong),
        _ => Err(usage("--crossfeed must be off, subtle, natural or strong")),
    }
}

pub fn parse_dsp_update(current: DspSettings, args: &[String]) -> Result<DspSettings, Failure> {
    let shape = "dsp set [--crossfeed off|subtle|natural|strong] [--limiter on|off] [--mono on|off] [--invert-phase on|off] [--loudness on|off] [--swap-channels on|off]";
    if args.is_empty() {
        return Err(usage(shape));
    }
    let mut next = current;
    let mut i = 0;
    while i < args.len() {
        let Some(value) = args.get(i + 1) else { return Err(usage(shape)) };
        match args[i].as_str() {
            "--crossfeed" => next.crossfeed = parse_crossfeed(value)?,
            "--limiter" => next.limiter = parse_bool(value, "--limiter")?,
            "--mono" => next.mono = parse_bool(value, "--mono")?,
            "--invert-phase" => next.invert_phase = parse_bool(value, "--invert-phase")?,
            "--loudness" => next.loudness = parse_bool(value, "--loudness")?,
            "--swap-channels" => next.swap_channels = parse_bool(value, "--swap-channels")?,
            _ => return Err(usage(shape)),
        }
        i += 2;
    }
    Ok(next)
}

pub fn dsp(client: &mut Connection, args: &[String]) -> Result<Value, Failure> {
    match args.first().map(String::as_str) {
        None => Ok(json!(client.dsp().map_err(|e| case_failure("audio/dsp/get", e))?)),
        Some("set") => {
            let current = client.dsp().map_err(|e| case_failure("audio/dsp/get", e))?;
            let next = parse_dsp_update(current, &args[1..])?;
            client.set_dsp(next).map_err(|e| case_failure("audio/dsp/set", e))?;
            Ok(json!(next))
        }
        _ => Err(usage(
            "dsp [set [--crossfeed off|subtle|natural|strong] [--limiter on|off] [--mono on|off] [--invert-phase on|off] [--loudness on|off] [--swap-channels on|off]]",
        )),
    }
}

fn parse_class_h(text: &str) -> Result<ClassHMode, Failure> {
    match text {
        "adaptive-hv" => Ok(ClassHMode::AdaptiveHv),
        "low-power" => Ok(ClassHMode::LowPowerLv),
        "fixed-hv" => Ok(ClassHMode::FixedHv),
        _ => Err(usage("--class-h must be adaptive-hv, low-power or fixed-hv")),
    }
}

pub fn parse_dac_extras_update(current: DacExtras, args: &[String]) -> Result<DacExtras, Failure> {
    let shape = "dac-extras set [--hpf on|off] [--deemp on|off] [--line-out on|off] [--class-h adaptive-hv|low-power|fixed-hv] [--auto-uac on|off] [--stay-awake on|off]";
    if args.is_empty() {
        return Err(usage(shape));
    }
    let mut next = current;
    let mut i = 0;
    while i < args.len() {
        let Some(value) = args.get(i + 1) else { return Err(usage(shape)) };
        match args[i].as_str() {
            "--hpf" => next.high_pass_filter = parse_bool(value, "--hpf")?,
            "--deemp" => next.de_emphasis = parse_bool(value, "--deemp")?,
            "--line-out" => next.line_out = parse_bool(value, "--line-out")?,
            "--class-h" => next.class_h = parse_class_h(value)?,
            "--auto-uac" => next.auto_uac_on_jack = parse_bool(value, "--auto-uac")?,
            "--stay-awake" => next.stay_awake_while_active = parse_bool(value, "--stay-awake")?,
            _ => return Err(usage(shape)),
        }
        i += 2;
    }
    Ok(next)
}

pub fn dac_extras(client: &mut Connection, args: &[String]) -> Result<Value, Failure> {
    match args.first().map(String::as_str) {
        None => Ok(json!(client.dac_extras().map_err(|e| case_failure("audio/dac-extras/get", e))?)),
        Some("set") => {
            let current = client.dac_extras().map_err(|e| case_failure("audio/dac-extras/get", e))?;
            let next = parse_dac_extras_update(current, &args[1..])?;
            client.set_dac_extras(next).map_err(|e| case_failure("audio/dac-extras/set", e))?;
            Ok(json!(next))
        }
        _ => Err(usage(
            "dac-extras [set [--hpf on|off] [--deemp on|off] [--line-out on|off] [--class-h adaptive-hv|low-power|fixed-hv] [--auto-uac on|off] [--stay-awake on|off]]",
        )),
    }
}

pub fn parse_eq_band(text: &str) -> Result<EqBand, Failure> {
    let shape = "band must be KIND:FREQ_HZ:Q:GAIN_DB (KIND: peak|lowshelf|highshelf|lowpass|highpass)";
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() != 4 {
        return Err(usage(shape));
    }
    let kind = match parts[0] {
        "peak" => FilterKind::Peak,
        "lowshelf" => FilterKind::LowShelf,
        "highshelf" => FilterKind::HighShelf,
        "lowpass" => FilterKind::LowPass,
        "highpass" => FilterKind::HighPass,
        _ => return Err(usage(shape)),
    };
    let frequency_hz = parts[1].parse::<u32>().map_err(|_| usage(shape))?;
    let q = parts[2].parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| usage(shape))?;
    let q_milli_f = (q * 1_000.0).round();
    if !(f64::from(EqBand::MIN_Q_MILLI)..=f64::from(EqBand::MAX_Q_MILLI)).contains(&q_milli_f) {
        return Err(usage("Q must be in 0.1..20.0"));
    }
    let gain_centi_db = parse_centi_db(parts[3], -EqBand::MAX_GAIN_CENTI_DB, EqBand::MAX_GAIN_CENTI_DB, "gain")?;
    let band = EqBand { kind, frequency_hz, q_milli: q_milli_f as u16, gain_centi_db };
    if !band.is_valid() {
        return Err(usage("band parameters out of valid range (10..40000 Hz, Q 0.1..20, gain -18..18 dB)"));
    }
    Ok(band)
}

pub fn parse_eq_slot(text: &str) -> Result<u8, Failure> {
    text.parse::<u8>().ok().filter(|slot| *slot < EQ_SLOTS).ok_or_else(|| usage("slot must be 0..3"))
}

pub fn parse_eq_set(args: &[String]) -> Result<EqSlotUpdate, Failure> {
    let shape = "eq set SLOT PREAMP_DB [KIND:FREQ_HZ:Q:GAIN_DB ...]";
    if args.len() < 2 || args.len() > 2 + EQ_MAX_BANDS {
        return Err(usage(shape));
    }
    let slot = parse_eq_slot(&args[0])?;
    let preamp_centi_db = parse_centi_db(&args[1], EqProfile::MIN_PREAMP_CENTI_DB, 0, "preamp")?;
    let mut bands = heapless::Vec::new();
    for spec in &args[2..] {
        bands.push(parse_eq_band(spec)?).map_err(|_| usage("at most 10 EQ bands are allowed"))?;
    }
    Ok(EqSlotUpdate { slot, profile: EqProfile { preamp_centi_db, bands } })
}

pub fn eq(client: &mut Connection, args: &[String]) -> Result<Value, Failure> {
    match args.first().map(String::as_str) {
        None => {
            let status = client.audio_status().map_err(|e| case_failure("audio/status", e))?;
            let mut slots = Vec::new();
            for slot in 0..EQ_SLOTS {
                let profile = client.eq(slot).map_err(|e| case_failure("audio/eq/get", e))?;
                slots.push(json!({"slot": slot, "profile": profile}));
            }
            Ok(json!({"active": status.eq_active, "slots": slots}))
        }
        Some("set") => {
            let update = parse_eq_set(&args[1..])?;
            client.set_eq(update.clone()).map_err(|e| case_failure("audio/eq/set", e))?;
            Ok(json!(update))
        }
        Some("select") => {
            let selection = match args.get(1).map(String::as_str) {
                Some("off" | "none") if args.len() == 2 => None,
                Some(slot) if args.len() == 2 => Some(parse_eq_slot(slot)?),
                _ => return Err(usage("eq select SLOT|off")),
            };
            client.select_eq(selection).map_err(|e| case_failure("audio/eq/select", e))?;
            Ok(json!({"selected": selection}))
        }
        Some(slot) if args.len() == 1 => {
            let slot = parse_eq_slot(slot)?;
            let profile = client.eq(slot).map_err(|e| case_failure("audio/eq/get", e))?;
            Ok(json!({"slot": slot, "profile": profile}))
        }
        _ => Err(usage("eq [SLOT] | eq set SLOT PREAMP_DB [BAND ...] | eq select SLOT|off")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn tone_and_silence_arguments_have_bring_up_defaults() {
        let tone = parse_start(&words("48000"), true).unwrap();
        assert_eq!(tone, AudioStart { rate_hz: 48_000, channel: AudioChannel::Both, level_centi_dbfs: -2_000, tone_hz: 1_000 });
        let left = parse_start(&words("44100 left 500 -40.5"), true).unwrap();
        assert_eq!(left, AudioStart { rate_hz: 44_100, channel: AudioChannel::Left, level_centi_dbfs: -4_050, tone_hz: 500 });
        assert_eq!(parse_start(&words("96000 right"), true).unwrap().channel, AudioChannel::Right);
        let silence = parse_start(&words("48000"), false).unwrap();
        assert_eq!(silence, AudioStart { rate_hz: 48_000, channel: AudioChannel::Both, level_centi_dbfs: 0, tone_hz: 0 });
        for bad in
            ["", "50000", "48000 centre", "48000 left 0", "48000 left 30000", "48000 left 1000 3", "48000 left 1000 -97", "x"]
        {
            assert_eq!(parse_start(&words(bad), true).unwrap_err().code, 2, "{bad}");
        }
        assert_eq!(parse_start(&words("48000 left"), false).unwrap_err().code, 2);
    }

    fn sample(t_ms: f64, frame: u32, wraps: u32, cycles: u32, ring_bytes: u32) -> Sample {
        let mut monitor = vec![0; usize::from(AUDIO_MONITOR_WORDS)];
        monitor[AUDIO_MONITOR_FRAME] = frame;
        monitor[AUDIO_MONITOR_WRAPS] = wraps;
        monitor[AUDIO_MONITOR_CYCLES] = cycles;
        Sample { host_us: t_ms * 1_000.0, monitor, remaining: ring_bytes - frame * FRAME_BYTES as u32 - 1 }
    }

    #[test]
    fn cursor_samples_give_frames_per_second_and_wraps() {
        let ring = 2_016u32;
        let bytes = ring * FRAME_BYTES as u32;
        let samples: Vec<Sample> = (0..=100u32)
            .map(|i| {
                let played = 480 * i;
                sample(f64::from(i) * 10.0, played % ring, played / ring, (i * 600_000).wrapping_add(u32::MAX - 5), bytes)
            })
            .collect();
        let summary = analyse(&samples, ring, 48_000, 60_000_000);
        assert_eq!(summary["samples"], 101);
        assert_eq!(summary["firmware"]["frames"], 48_000.0);
        assert_eq!(summary["firmware"]["wraps"], 23);
        assert_eq!(summary["firmware"]["by_host_clock"]["frames_per_second"], 48_000.0);
        assert_eq!(summary["firmware"]["by_mcycle"]["frames_per_second"], 48_000.0);
        assert_eq!(summary["registers"]["frames"], 48_000);
        assert_eq!(summary["registers"]["wraps"], 23);
        assert_eq!(summary["registers"]["wraps_trustworthy"], true);
        assert_eq!(summary["decoding"]["learned"], false);
        assert_eq!(summary["counters"]["underruns"], 0);
        let stock = Decoding::REMAINING_BYTES;
        assert_eq!(register_frame(bytes, stock, 0xffff), None);
        assert_eq!(register_frame(bytes, stock, bytes - 1), Some(0));
        let up = Decoding { direction: Direction::Up, unit_bytes: 1, bias: 1 };
        assert_eq!(register_frame(bytes, up, 8 * 5 - 1), Some(5));
        let mut learned = samples[0].monitor.clone();
        learned[AUDIO_MONITOR_DECODING] = up.word();
        learned[AUDIO_MONITOR_PROBE_RATE] = 384_000;
        assert_eq!(describe_decoding(&learned)["direction"], "up");
        learned[AUDIO_MONITOR_DECODING] = ProbeFault::Pace.code() << 24;
        assert_eq!(describe_decoding(&learned)["probe_fault"], "Pace");
        assert_eq!(analyse(&[], ring, 48_000, 60_000_000), json!({"samples": 0}));
    }

    #[test]
    fn output_and_eq_arguments_parse_valid_ranges_and_refuse_the_rest() {
        let defaults = OutputSettings {
            filter: DacFilter::FastLowLatency,
            volume_limit_centi_db: 0,
            balance_centi_db: 0,
            dsd_direct: false,
        };
        let updated = parse_output_update(defaults, &words("--filter nos --limit -6.5 --balance 2.5 --dsd-direct on")).unwrap();
        assert_eq!(
            updated,
            OutputSettings {
                filter: DacFilter::NonOversampling,
                volume_limit_centi_db: -650,
                balance_centi_db: 250,
                dsd_direct: true,
            }
        );
        for bad in ["", "--filter bad", "--limit 1", "--balance 7", "--dsd-direct maybe"] {
            assert_eq!(parse_output_update(defaults, &words(bad)).unwrap_err().code, 2, "{bad}");
        }
        let eq = parse_eq_set(&words("0 -6.0 lowshelf:80:0.707:5.0 peak:1000:1.5:-3.5 highshelf:8000:0.707:2")).unwrap();
        assert_eq!(eq.slot, 0);
        assert_eq!(eq.profile.preamp_centi_db, -600);
        assert_eq!(eq.profile.bands.len(), 3);
        assert_eq!(
            eq.profile.bands[0],
            EqBand { kind: FilterKind::LowShelf, frequency_hz: 80, q_milli: 707, gain_centi_db: 500 }
        );
        for bad in ["0", "4 0", "0 1", "0 -25", "0 0 peak:1000:1", "0 0 nope:1000:1:3", "0 0 peak:1000:1:20"] {
            assert_eq!(parse_eq_set(&words(bad)).unwrap_err().code, 2, "{bad}");
        }
        let dsp = parse_dsp_update(
            DspSettings::default(),
            &words("--crossfeed natural --limiter on --mono off --invert-phase on --loudness on --swap-channels on"),
        )
        .unwrap();
        assert_eq!(
            dsp,
            DspSettings {
                crossfeed: CrossfeedMode::Natural,
                limiter: true,
                mono: false,
                invert_phase: true,
                loudness: true,
                swap_channels: true,
            }
        );
        for bad in ["", "--crossfeed extreme", "--limiter maybe", "--mono"] {
            assert_eq!(parse_dsp_update(DspSettings::default(), &words(bad)).unwrap_err().code, 2, "{bad}");
        }
    }
}
