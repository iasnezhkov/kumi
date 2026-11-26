// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![no_std]
#![no_main]

mod app;
mod audio;
mod board;
#[cfg(feature = "lab")]
mod lab;
mod uac;
mod usb;

use core::arch::{asm, global_asm};
use core::future::Future;
use core::panic::PanicInfo;
use core::pin::pin;
use core::task::{Context, Waker};
use embassy_usb::class::hid;
use heapless::Deque;
use kumi_audio::{board as pins, regs};
use kumi_chip::power::CpuProfile;
use kumi_control::SaveTrigger;
use kumi_icd::{
    Jack, RebootTarget, USB_PRODUCT_ID, USB_VENDOR_ID, VENDOR_INTERFACE_CLASS, VENDOR_INTERFACE_PROTOCOL,
    VENDOR_INTERFACE_SUBCLASS,
};
use kumi_keymap::{Engine, Input, Keymap};

global_asm!(
    ".section .text.entry, \"ax\"",
    ".global _start",
    "_start:",
    "csrci mstatus, 8",
    "csrw mie, zero",
    "li t0, 0x2000",
    "csrs mstatus, t0",
    "csrw fcsr, zero",
    "fence.i",
    "csrr t0, 0x7ca",
    "ori t0, t0, 1",
    "csrw 0x7ca, t0",
    "mv s1, a0",
    ".option push",
    ".option norelax",
    "la gp, __global_pointer$",
    ".option pop",
    "la sp, __stack_top",
    "la t0, kumi_trap",
    "csrw mtvec, t0",
    "la t0, __bss_start",
    "la t1, __bss_end",
    "1:",
    "bgeu t0, t1, 2f",
    "sw zero, 0(t0)",
    "addi t0, t0, 4",
    "j 1b",
    "2:",
    "mv a0, s1",
    "call kumi_main",
    ".balign 4",
    ".global kumi_trap",
    "kumi_trap:",
    "mv a4, sp",
    "mv a3, ra",
    "csrr a0, mcause",
    "csrr a1, mepc",
    "csrr a2, mtval",
    "la sp, __trap_stack_top",
    "la t0, kumi_chip_fault_reset_normal",
    "csrw mtvec, t0",
    "call kumi_trap_main",
    ".global kumi_fault",
    "kumi_fault:",
    "j kumi_chip_fault_reset_normal",
);

struct CriticalSection;
critical_section::set_impl!(CriticalSection);

unsafe impl critical_section::Impl for CriticalSection {
    unsafe fn acquire() -> bool {
        let old: u32;
        unsafe { asm!("csrrci {0}, mstatus, 8", out(reg) old, options(nomem, nostack)) };
        old & 8 != 0
    }

    unsafe fn release(restore: bool) {
        if restore {
            unsafe { asm!("csrsi mstatus, 8", options(nomem, nostack)) };
        }
    }
}

const USB_SPEED: usb::Speed = if option_env!("KUMI_USB_FULL_SPEED").is_some() { usb::Speed::Full } else { usb::Speed::High };
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn kumi_poll() {
    unsafe { asm!("nop", options(nomem, nostack)) };
}

fn mark(stage: u16) {
    board::mark(stage);
    #[cfg(feature = "lab")]
    lab::mark(stage);
}

const LINK_LOG_RECORDS: usize = 64;
const LINK_LOG_BODY: usize = 12;

#[unsafe(no_mangle)]
static mut KUMI_LINK_LOG: [[u8; 4 + LINK_LOG_BODY]; LINK_LOG_RECORDS] = [[0; 4 + LINK_LOG_BODY]; LINK_LOG_RECORDS];
static mut LINK_LOG_NEXT: usize = 0;

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
fn log_link(direction: u8, group: u8, command: u8, body: &[u8]) {
    let next = unsafe { (&raw const LINK_LOG_NEXT).read() };
    if next >= LINK_LOG_RECORDS {
        return;
    }
    let mut record = [0u8; 4 + LINK_LOG_BODY];
    record[..4].copy_from_slice(&[direction, group, command, body.len() as u8]);
    let kept = body.len().min(LINK_LOG_BODY);
    record[4..4 + kept].copy_from_slice(&body[..kept]);
    unsafe {
        (&raw mut KUMI_LINK_LOG).cast::<[u8; 4 + LINK_LOG_BODY]>().add(next).write(record);
        (&raw mut LINK_LOG_NEXT).write(next + 1);
    }
}

fn settings_has_mouse(settings: &kumi_settings::StoredSettings) -> bool {
    settings.keymap.iter().any(|entry| matches!(entry.action, kumi_icd::KeyAction::Mouse(_)))
        || settings.combos.iter().flatten().any(|combo| matches!(combo.action, kumi_icd::KeyAction::Mouse(_)))
}

fn rebuild(keymap: &mut Keymap, shared: &app::Shared) {
    let controls = shared.controls.borrow();
    let settings = controls.settings();
    keymap.clear();
    keymap.clear_macros();
    keymap.clear_combos();
    keymap.set_config(settings.key_config);
    for (slot, program) in settings.macros.iter().enumerate() {
        let _ = keymap.set_macro(slot as u8, program.clone());
    }
    for (slot, combo) in settings.combos.iter().enumerate() {
        let _ = keymap.set_combo(slot as u8, *combo);
    }
    for entry in &settings.keymap {
        let _ = keymap.set(entry.layer, entry.source, entry.action);
    }
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.cold")]
#[inline(never)]
extern "C" fn kumi_main(boot_arg: u32) -> ! {
    let info = board::adopt_boot_info(boot_arg);
    #[cfg(feature = "lab")]
    {
        lab::capture_boot();
        lab::start(board::loaded_by_host(), info.map_or(0, |i| i.boot_count));
    }
    mark(kumi_diag::stage::FW_ENTRY);
    assert!(board::usb_init(), "USB clock initialization failed");
    mark(kumi_diag::stage::FW_CLOCKS);
    let (flash, jedec) = board::open_flash();
    let mut storage = board::storage(flash);
    let safe_mode = info.is_some_and(|info| info.safe_mode);
    let settings = if safe_mode { kumi_settings::StoredSettings::default() } else { storage.load_settings() };
    mark(kumi_diag::stage::FW_STORAGE);
    board::uart_init();
    board::keep_awake();
    board::arm_watchdog();
    mark(kumi_diag::stage::FW_UART);
    let mut jack = kumi_audio::jack::Detector::new();
    let initial_jack = jack.init_and_sample(&mut board::Mmio, 50);
    audio::power_down_dacs();
    regs::apply(&mut board::Mmio, &pins::POWER_OFF);
    board::set_cpu_profile(CpuProfile::Idle90);
    board::enable_idle_wake();
    let audio_config = uac::config(USB_SPEED);
    let shared = app::Shared::new(settings, storage, safe_mode, audio_config);
    shared.uac.borrow_mut().set_jack(initial_jack);
    let state = usb::State::new();
    let driver = usb::Driver::new(&state, USB_SPEED);
    let serial_bytes = board::serial().map(kumi_chip::efuse::format_serial_hex);
    let mut config = embassy_usb::Config::new(USB_VENDOR_ID, USB_PRODUCT_ID);
    config.manufacturer = Some("Kumi");
    config.product = Some(if board::LAB { "Kumi Lab" } else { "Kumi" });
    config.serial_number = serial_bytes.as_ref().and_then(|bytes| core::str::from_utf8(bytes).ok());
    config.supports_remote_wakeup = true;
    config.max_packet_size_0 = 64;
    let mut config_descriptor = [0u8; 512];
    let mut bos = [0u8; 96];
    let mut msos = [0u8; 128];
    let mut control = [0u8; 256];
    let mut builder = embassy_usb::Builder::new(driver, config, &mut config_descriptor, &mut bos, &mut msos, &mut control);
    let mut lifecycle = app::Lifecycle(&shared);
    builder.handler(&mut lifecycle);
    let audio_function =
        board::raw_access() || !shared.controls.borrow().dac_extras().auto_uac_on_jack || initial_jack == Jack::Headphones;
    let mut audio_requests = uac::Requests(&shared, &state);
    if audio_function {
        builder.handler(&mut audio_requests);
    }
    builder.msos_descriptor(embassy_usb::msos::windows_version::WIN10, 0x20);
    let mouse_function = settings_has_mouse(shared.controls.borrow().settings());
    let mut hid_state = hid::State::new();
    let mut handler = app::HidControl(&shared, &state, mouse_function);
    let hid = hid::HidWriter::<_, 16>::new(
        &mut builder,
        &mut hid_state,
        hid::Config {
            report_descriptor: if mouse_function {
                kumi_keymap::hid::REPORT_DESCRIPTOR
            } else {
                kumi_keymap::hid::REPORT_DESCRIPTOR_NO_MOUSE
            },
            request_handler: Some(&mut handler),
            poll_ms: 1,
            max_packet_size: 16,
            hid_subclass: hid::HidSubclass::Boot,
            hid_boot_protocol: hid::HidBootProtocol::Keyboard,
        },
    );
    let mut function = builder.function(VENDOR_INTERFACE_CLASS, VENDOR_INTERFACE_SUBCLASS, VENDOR_INTERFACE_PROTOCOL);
    function.msos_feature(embassy_usb::msos::CompatibleIdFeatureDescriptor::new("WINUSB", ""));
    let mut interface = function.interface();
    let mut alt = interface.alt_setting(VENDOR_INTERFACE_CLASS, VENDOR_INTERFACE_SUBCLASS, VENDOR_INTERFACE_PROTOCOL, None);
    alt.bos_capability(
        0x05,
        &[
            0x00, 0x38, 0xb6, 0x08, 0x34, 0xa9, 0x09, 0xa0, 0x47, 0x8b, 0xfd, 0xa0, 0x76, 0x88, 0x15, 0xb6, 0x65, 0x00, 0x01,
            0x21, 0x01,
        ],
    );
    let vendor_in = alt.endpoint_bulk_in(Some(0x82.into()), USB_SPEED.bulk_packet());
    let vendor_out = alt.endpoint_bulk_out(Some(0x03.into()), USB_SPEED.bulk_packet());
    drop(function);
    let mut audio = if audio_function {
        let Some((audio_out, audio_feedback)) = uac::build(&mut builder, &audio_config) else { panic!("audio function") };
        shared.uac.borrow_mut().set_present();
        Some(uac::Stream::new(audio_out, audio_feedback, &audio_config))
    } else {
        None
    };
    let mut device = builder.build();
    mark(kumi_diag::stage::FW_USB);
    let mut usb = pin!(device.run());
    let mut commands = pin!(app::rpc(vendor_out, vendor_in, &shared, jedec));
    let mut reports = pin!(app::hid_reports(hid, &shared, &state, mouse_function));
    mark(if safe_mode { kumi_diag::stage::FW_SAFE_MODE } else { kumi_diag::stage::FW_RUNNING });
    run_loop(
        usb.as_mut(),
        commands.as_mut(),
        reports.as_mut(),
        &shared,
        &state,
        &mut audio,
        audio_function,
        mouse_function,
        &mut jack,
    )
}

#[unsafe(link_section = ".text.hot")]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn run_loop<'a, U, C, R>(
    mut usb: core::pin::Pin<&mut U>,
    mut commands: core::pin::Pin<&mut C>,
    mut reports: core::pin::Pin<&mut R>,
    shared: &app::Shared,
    state: &usb::State,
    audio: &mut Option<uac::Stream<'a>>,
    audio_function: bool,
    mouse_function: bool,
    jack: &mut kumi_audio::jack::Detector,
) -> !
where
    U: Future,
    C: Future,
    R: Future,
{
    let mut context = Context::from_waker(Waker::noop());
    let mut receiver = kumi_kbdlink::Receiver::new();
    let mut input = Input::default();
    let mut engine = Engine::default();
    let mut keymap = Keymap::new(kumi_keymap::STOCK_CONSUMER_USAGES);
    rebuild(&mut keymap, shared);
    let mut keymap_dirty = true;
    let mut health = kumi_runtime_core::Health::default();
    let mut jack_tick = 0u32;
    let mut usb_reboot_at: Option<u32> = None;
    let mut tx = Deque::<u8, 256>::new();
    const SLEEP_BOOT_GRACE_US: u64 = 15_000_000;
    const SLEEP_SOF_TIMEOUT_US: u64 = 5_000_000;
    const WAKEUP_GRACE_US: u64 = 2_000_000;
    const IDLE_SLEEP_MS: u32 = 30_000;
    let boot_us = board::monotonic_us();
    let mut last_sof_us = boot_us;
    let mut wakeup_grace_until_us = 0u64;
    let mut boot_grace = true;
    let mut was_suspended = false;
    let mut remote_wakeup_sent = false;
    let mut idle_streak = 0u8;
    let mut last_rpc = shared.rpc_activity.get();
    let mut frame = kumi_chip::regs::usb_frame();
    let mut counting = false;
    loop {
        board::kick_watchdog();
        let _ = usb.as_mut().poll(&mut context);
        let _ = commands.as_mut().poll(&mut context);
        let _ = reports.as_mut().poll(&mut context);
        let mut loop_busy = false;
        let rpc_now = shared.rpc_activity.get();
        if rpc_now != last_rpc {
            last_rpc = rpc_now;
            loop_busy = true;
        }
        let now = kumi_chip::regs::usb_frame();
        let now_us = board::monotonic_us();
        let mono_ms = (now_us / 1_000) as u32;
        if boot_grace && now_us.saturating_sub(boot_us) >= SLEEP_BOOT_GRACE_US {
            boot_grace = false;
        }
        if now != frame {
            last_sof_us = now_us;
            boot_grace = false;
            if shared.suspended.get() {
                shared.suspended.set(false);
                remote_wakeup_sent = false;
            }
        }
        let mask = kumi_chip::regs::USB_FRAME_MASK as u16;
        let configured = shared.configured.get();
        if configured && counting {
            shared.milliseconds.set(shared.milliseconds.get().wrapping_add(u32::from(now.wrapping_sub(frame) & mask)));
        }
        counting = configured;
        board::set_uptime(shared.milliseconds.get());
        shared.usb_high_speed.set(state.high_speed());
        frame = now;
        let (settings_changed, keymap_changed) = shared.note_changes();
        if settings_changed {
            loop_busy = true;
        }
        if keymap_changed {
            rebuild(&mut keymap, shared);
            engine.clear_toggled();
            keymap_dirty = true;
        }
        let streaming = audio.as_ref().is_some_and(uac::Stream::streaming);
        let _ = shared.persist_settings(shared.take_flush(), streaming);
        let want_audio = board::raw_access()
            || !shared.controls.borrow().dac_extras().auto_uac_on_jack
            || shared.uac.borrow().jack() == Jack::Headphones;
        let need_reenum = !board::raw_access() && (keymap.has_mouse_action() != mouse_function || want_audio != audio_function);
        let can_reenum = !streaming
            && shared.developer().is_err()
            && !shared.storage.borrow().update_in_progress()
            && !shared.audio.borrow().busy();
        if !need_reenum {
            usb_reboot_at = None;
        } else if can_reenum && !shared.autosave_pending() {
            let _ = usb_reboot_at.get_or_insert_with(|| mono_ms.wrapping_add(200));
        }
        if let Some(at) = usb_reboot_at
            && mono_ms.wrapping_sub(at) < 0x8000_0000
            && can_reenum
        {
            let _ = shared.note_changes();
            let _ = shared.persist_settings(SaveTrigger::LinkDown, false);
            if !shared.autosave_pending() {
                board::reset(RebootTarget::Normal);
            }
        }
        if health.due(shared.configured.get(), shared.milliseconds.get()) {
            board::kick_watchdog();
            let mut storage = shared.storage.borrow_mut();
            if storage.reached_health() != Err(kumi_icd::CaseError::Busy) {
                health.finish();
                let _ = storage.log_crash(&board::last_crash_area());
                mark(kumi_diag::stage::FW_HEALTHY);
            }
            board::kick_watchdog();
            loop_busy = true;
        }
        for _ in 0..32 {
            let Some(byte) = board::uart_rx() else { break };
            loop_busy = true;
            shared.mirror_byte(byte);
            let bad_lengths = receiver.stats.bad_lengths;
            if let Some(frame) = receiver.push(byte) {
                log_link(b'R', frame.group, frame.command, frame.body);
                #[cfg(feature = "lab")]
                lab::link(true, frame.group, frame.command, frame.body);
                let mut status = shared.status.get();
                status.frames = status.frames.wrapping_add(1);
                if frame.checksum() == kumi_kbdlink::Checksum::Mismatch {
                    status.bad_frames = status.bad_frames.wrapping_add(1);
                    #[cfg(feature = "lab")]
                    lab::link_error(lab::LINK_CHECKSUM, status.frames);
                } else {
                    status.link_up = true;
                    if let kumi_kbdlink::Message::Hid(report) = kumi_kbdlink::decode(&frame) {
                        shared.touch_activity();
                        if shared.suspended.get() && shared.remote_wakeup_enabled.get() && !remote_wakeup_sent {
                            remote_wakeup_sent = true;
                            wakeup_grace_until_us = now_us.saturating_add(WAKEUP_GRACE_US);
                            board::set_cpu_profile(CpuProfile::Idle90);
                            board::keep_awake();
                            usb::trigger_remote_wakeup();
                        }
                        kumi_kbdlink::apply_hid(&mut input, &report);
                        let out = engine.process_at(&keymap, &input, mono_ms);
                        shared.apply_case_hotkeys(&out.case_hotkeys);
                        shared.report(&out);
                        status.active_layer = engine.top_layer();
                        status.caps_word = engine.caps_word();
                    } else {
                        status.controller_commands = status.controller_commands.wrapping_add(1);
                    }
                    let link = shared.uac.borrow().link();
                    #[cfg(feature = "lab")]
                    let link = lab::link_state(link);
                    let (sample_rate_hz, stream, pcm_bits) = link;
                    let eq_profile = shared.uac.borrow().eq_profile();
                    let state = kumi_kbdlink::CaseState {
                        sample_rate_hz,
                        stream,
                        pcm_bits,
                        legacy_volume: 0,
                        capture_trim: 0,
                        eq_profile,
                        mode_c: 0,
                        mode_d: 0,
                        mode_e: 0,
                        usb_mode: 0,
                    };
                    if let Some(reply) = kumi_kbdlink::reply(&frame, &state)
                        && tx.capacity() - tx.len() >= reply.len()
                    {
                        log_link(b'T', reply[1], reply[2], &reply[4..reply.len() - 2]);
                        #[cfg(feature = "lab")]
                        lab::link(false, reply[1], reply[2], &reply[4..reply.len() - 2]);
                        for byte in reply {
                            let _ = tx.push_back(byte);
                        }
                    }
                }
                shared.status.set(status);
            }
            let mut status = shared.status.get();
            status.bad_frames = status.bad_frames.wrapping_add(receiver.stats.bad_lengths.wrapping_sub(bad_lengths));
            shared.status.set(status);
            #[cfg(feature = "lab")]
            if receiver.stats.bad_lengths != bad_lengths {
                lab::link_error(lab::LINK_BAD_LENGTH, receiver.stats.bad_lengths);
            }
        }
        shared.tick_mirror();
        let ms = shared.milliseconds.get();
        if mono_ms.wrapping_sub(jack_tick) >= 10 {
            jack_tick = mono_ms;
            if let Some(state) = jack.poll(&mut board::Mmio) {
                shared.uac.borrow_mut().set_jack(state);
                loop_busy = true;
            }
        }
        shared.audio.borrow_mut().poll(shared.developer().is_ok(), mono_ms);
        if let Some(audio) = audio.as_mut() {
            audio.poll(shared);
        }
        let suspended = shared.suspended.get();
        if !suspended {
            remote_wakeup_sent = false;
        }
        if !was_suspended && suspended {
            if let Some(audio) = audio.as_mut() {
                audio.suspend(shared);
            }
            if !shared.audio.borrow().busy() {
                regs::apply(&mut board::Mmio, &pins::POWER_OFF);
                board::set_cpu_profile(CpuProfile::Suspend30);
            }
        }
        if was_suspended && !suspended {
            if !shared.audio.borrow().busy() {
                board::set_cpu_profile(CpuProfile::Idle90);
            }
            if let Some(audio) = audio.as_mut() {
                audio.resume(shared);
            }
            last_sof_us = now_us;
            shared.touch_activity();
        }
        if was_suspended != suspended {
            loop_busy = true;
        }
        was_suspended = suspended;
        let audio_active = audio.as_ref().is_some_and(uac::Stream::streaming) || shared.audio.borrow().busy();
        if audio_active {
            shared.touch_activity();
        }
        let audio_busy = audio_active || audio.as_ref().is_some_and(uac::Stream::active);
        #[cfg(feature = "lab")]
        let hold_awake = lab::hold_awake();
        #[cfg(not(feature = "lab"))]
        let hold_awake = false;
        let waking = now_us < wakeup_grace_until_us;
        let usb_active = !suspended && (boot_grace || now_us.saturating_sub(last_sof_us) < SLEEP_SOF_TIMEOUT_US);
        let recently_active = boot_grace
            || audio_busy
            || shared.developer().is_ok()
            || ms.wrapping_sub(shared.last_activity_ms.get()) < IDLE_SLEEP_MS;
        let stay_awake = shared.controls.borrow().dac_extras().stay_awake_while_active;
        let want_awake = hold_awake || waking || (usb_active && (board::raw_access() || stay_awake || recently_active));
        if !audio_busy && want_awake != board::is_awake() {
            board::set_awake(want_awake);
        }
        while !tx.is_full() {
            let Some(byte) = shared.pop_uart_tx() else { break };
            let _ = tx.push_back(byte);
            loop_busy = true;
        }
        for _ in 0..16 {
            let Some(byte) = tx.front().copied() else { break };
            if !board::uart_tx(byte) {
                break;
            }
            tx.pop_front();
            loop_busy = true;
        }
        if keymap_dirty {
            keymap_dirty = false;
            loop_busy = true;
            let out = engine.process_at(&keymap, &input, mono_ms);
            shared.apply_case_hotkeys(&out.case_hotkeys);
            shared.report(&out);
            let mut status = shared.status.get();
            status.active_layer = engine.top_layer();
            status.caps_word = engine.caps_word();
            shared.status.set(status);
        } else if let Some(out) = engine.tick(&keymap, mono_ms) {
            loop_busy = true;
            shared.apply_case_hotkeys(&out.case_hotkeys);
            shared.report(&out);
            let mut status = shared.status.get();
            status.active_layer = engine.top_layer();
            status.caps_word = engine.caps_word();
            shared.status.set(status);
        }
        let can_sleep = !loop_busy
            && !audio_busy
            && usb_reboot_at.is_none()
            && !engine.has_fast_timers()
            && (configured || suspended || !boot_grace)
            && tx.is_empty()
            && !shared.has_pending_work()
            && !board::uart_rx_ready();
        if can_sleep {
            idle_streak = idle_streak.saturating_add(1);
        } else {
            idle_streak = 0;
        }
        #[cfg(feature = "lab")]
        lab::tick(ms);
        kumi_poll();
        if idle_streak >= 4 && !usb::prepare_idle_wake() {
            let (timer_fired, wfi_cycles) = board::enter_idle_wfi();
            if timer_fired {
                let measured_us = u64::from(wfi_cycles / board::cpu_mhz().max(1));
                if measured_us < 100 {
                    board::advance_monotonic_us(u64::from(board::WAKE_TIMER_MS) * 1_000);
                }
            }
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe { asm!("csrci mstatus, 8", options(nomem, nostack)) };
    board::crash(kumi_diag::Reason::Panic, kumi_diag::Registers::default(), info.message().as_str().unwrap_or("runtime panic"));
    board::reset(RebootTarget::Normal)
}

#[unsafe(no_mangle)]
extern "C" fn kumi_trap_main(mcause: u32, mepc: u32, mtval: u32, ra: u32, sp: u32) -> ! {
    board::crash(kumi_diag::Reason::Trap, kumi_diag::Registers { mcause, mepc, mtval, ra, sp }, "runtime fault");
    board::reset(kumi_icd::RebootTarget::Normal)
}
