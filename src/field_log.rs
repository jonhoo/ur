//! The field log: everything we'd see over the debug probe, kept in the SPI
//! flash, for watches without one (a sealed watch on a wrist).
//!
//! defmt already encodes every log message compactly into the RTT buffer
//! that a debug probe reads. In a field build (the `field-log` feature) this
//! module *is* that reader: it drains the buffer once a second and appends
//! the raw defmt frames to a ring of sectors in the SPI flash. A computer
//! later fetches them over Bluetooth (`scripts/fetch-log.py`, through the
//! service in [`crate::ble`]) and decodes them with `defmt-print` and the
//! ELF of the build that wrote them. So the field log costs no formatting
//! code on the watch, and holds what the probe would have shown, but for
//! its busiest lines, which a field build packs (see [`crate::log_pack`];
//! `scripts/expand_log.py` unpacks them after decoding).
//!
//! Not for builds read through a probe: the probe and this module would
//! each take whatever the other didn't.
//!
//! Layout: a ring of 4 KiB sectors ([`log_ring`] says where each is: 745
//! of them, in two stretches of the flash), each a 16-byte header (magic,
//! sequence number, image version, data length) followed by whole defmt
//! frames. Every boot starts a new sector; the oldest is erased to make
//! room. The length is written when a sector is closed (NOR flash can
//! still clear bits of the erased header), so a sector without one is either
//! the current one or was cut off by a reset; the reader then trims at the
//! last frame boundary.
//!
//! A sector is the log's only once we've erased it and written its header:
//! one that doesn't start with our magic is absent, to the writer (which
//! looks for the newest sector at boot) and to a fetcher alike. That's what
//! the ring's second stretch holds on a watch that ran InfiniTime (its file
//! system), or that updated from a build whose ring was the first stretch
//! only, until the writer first comes round to it.
//!
//! Also here, because they're about knowing what happened in the field: the
//! reset reason, and where the previous run panicked (see [`panic`]).

use core::cell::Cell;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use defmt::{info, warn};
use embassy_sync::blocking_mutex::Mutex as BlockingMutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
#[cfg(feature = "field-log")]
use embassy_time::Timer;

use crate::fault;
use crate::log_ring::{self, SECTORS, sector_address};
#[cfg(feature = "field-log")]
use crate::rtt_ring;
#[cfg(feature = "field-log")]
use crate::spi_flash::{self, SpiFlash};
use crate::spi_flash::{FLASH, SECTOR};

const MAGIC: u32 = u32::from_le_bytes(*b"urL1");
const HEADER: u32 = 16;

// Where the ring's sectors are is `log_ring`'s to say (and to check that
// each lies whole within one of its two stretches); what's checked here is
// how those stretches lie among the flash's other tenants, which that
// module, hardware-free, doesn't know. See `spi_flash` for the map.
const _: () = assert!(log_ring::SECTOR == SECTOR);
// The first stretch reaches to the chip's end and no further: the chip
// takes an address past its end as one from its start, where the
// bootloader's recovery firmware is. It starts after the System OFF record.
const _: () = assert!(log_ring::FIRST.end <= crate::spi_flash::SIZE);
const _: () = assert!(crate::system_off::RECORD.end <= log_ring::FIRST.start);
// The second lies between MCUBoot's secondary slot (an erase there would
// break an update half-way in, or the image a failed one reverts to) and
// the crash checkpoints.
const _: () =
    assert!(crate::dfu_slot::SLOT_START + crate::dfu_slot::SLOT_SIZE <= log_ring::SECOND.start);
const _: () = assert!(log_ring::SECOND.end <= crate::system_off::CHECKPOINTS.start);
// And a sector's header (with the length written into it later) lies
// within its first page, as one program must.
const _: () = assert!(HEADER <= crate::spi_flash::PAGE);

/// Where the log is being written: (sector index, bytes used including the
/// header), once the writer has started.
static CURSOR: BlockingMutex<CriticalSectionRawMutex, Cell<Option<(u32, u32)>>> =
    BlockingMutex::new(Cell::new(None));

// ==============================================================================
// Draining defmt's RTT buffer
// ==============================================================================
//
// Only in field builds; the rest of the module (reading the log back, reset
// reasons, panics) is in every build, so a probe-flashed build can still
// serve an earlier field build's log.

#[cfg(feature = "field-log")]
/// defmt-rtt's control block (`_SEGGER_RTT`), as laid out in defmt-rtt 1.3:
/// SEGGER's standard header with one up channel.
#[repr(C)]
struct RttHeader {
    id: [u8; 16],
    max_up_channels: u32,
    max_down_channels: u32,
    up: RttChannel,
}

#[cfg(feature = "field-log")]
#[repr(C)]
struct RttChannel {
    name: *const u8,
    buffer: *mut u8,
    size: u32,
    write: AtomicU32,
    read: AtomicU32,
    flags: AtomicU32,
}

#[cfg(feature = "field-log")]
unsafe extern "C" {
    static _SEGGER_RTT: RttHeader;
}

#[cfg(feature = "field-log")]
fn rtt() -> &'static RttChannel {
    // SAFETY: defmt-rtt defines this static; we only touch the fields a
    // debug probe would (the read cursor and the mode), which are atomics.
    unsafe { &_SEGGER_RTT.up }
}

#[cfg(feature = "field-log")]
/// How many bytes are waiting in the RTT buffer.
fn waiting() -> usize {
    let channel = rtt();
    let size = channel.size as usize;
    let write = channel.write.load(Ordering::Acquire) as usize;
    let read = channel.read.load(Ordering::Relaxed) as usize;
    rtt_ring::waiting(size, read, write)
}

#[cfg(feature = "field-log")]
/// The whole frames waiting in the RTT buffer, as many as fit in `room`
/// bytes, as the (at most two) pieces of the buffer they're in: the second
/// is where they continue after wrapping around its end. `None` if no whole
/// frame is waiting; empty pieces if the next one doesn't fit in `room`.
///
/// Read in place rather than copied out, which would take a buffer of its
/// own (it was 512 bytes in this task's static storage). They stay put until
/// [`consume`]d: defmt-rtt only writes at its write cursor, and stops short
/// of our read cursor.
fn frames(room: usize) -> Option<(&'static [u8], &'static [u8])> {
    let channel = rtt();
    let size = channel.size as usize;
    let write = channel.write.load(Ordering::Acquire) as usize;
    let read = channel.read.load(Ordering::Relaxed) as usize;
    let waiting = rtt_ring::waiting(size, read, write);
    // Which bytes to look at, and how many of them are whole frames, is
    // `rtt_ring`'s arithmetic (proved to stay between the cursors); the
    // bytes themselves are read here.
    //
    // SAFETY: Bytes between the read and write cursors are written, and
    // left alone by defmt-rtt until we move the read cursor past them
    // (which only `consume` does, after the caller is done with these).
    let byte = |at: usize| unsafe { channel.buffer.add(at).read() };
    let Some(fits) = rtt_ring::whole_frames(size, read, waiting, room, byte) else {
        if waiting == size - 1 {
            // Full, without a frame end: a frame too big for the buffer,
            // which we drop rather than stall on.
            consume(waiting);
        }
        return None;
    };
    let (in_first, in_second) = rtt_ring::pieces(size, read, fits);
    // SAFETY: As above: the first `fits` bytes after the read cursor.
    unsafe {
        Some((
            core::slice::from_raw_parts(channel.buffer.add(read), in_first),
            core::slice::from_raw_parts(channel.buffer, in_second),
        ))
    }
}

#[cfg(feature = "field-log")]
/// Marks `n` bytes (from [`frames`]) read, for defmt-rtt to reuse.
fn consume(n: usize) {
    let channel = rtt();
    let size = channel.size as usize;
    let read = channel.read.load(Ordering::Relaxed) as usize;
    channel
        .read
        .store(rtt_ring::after(size, read, n) as u32, Ordering::Release);
}

// ==============================================================================
// Writing to flash
// ==============================================================================

#[cfg(feature = "field-log")]
/// Finds the newest sector's sequence number, so this boot continues after
/// it.
///
/// Only sectors that start with our magic count, so whatever else the
/// flash holds where the ring now is (see the module's documentation) can't
/// pass for the newest, however large the number in its place. The magic is
/// all that's asked of a header, though: one whose length a reset cut off
/// mid-program (any value) is still the log's, and skipping it here would
/// hand its sequence number out twice.
async fn newest(flash: &mut SpiFlash) -> Option<(u32, u32)> {
    let mut newest: Option<(u32, u32)> = None;
    for index in 0..SECTORS {
        let mut header = [0u8; 8];
        flash.read(sector_address(index), &mut header).await;
        let [m0, m1, m2, m3, s0, s1, s2, s3] = header;
        if u32::from_le_bytes([m0, m1, m2, m3]) != MAGIC {
            continue;
        }
        let sequence = u32::from_le_bytes([s0, s1, s2, s3]);
        if newest.is_none_or(|(_, newest)| sequence > newest) {
            newest = Some((index, sequence));
        }
    }
    newest
}

#[cfg(feature = "field-log")]
/// Erases sector `index` and starts it with a header. Every sector the
/// writer uses goes through here first, so nothing is ever programmed over
/// what a sector held before (another firmware's data, in the ring's second
/// stretch).
async fn open(flash: &mut SpiFlash, index: u32, sequence: u32) -> Result<(), spi_flash::Error> {
    let address = sector_address(index);
    flash.erase(address..address + SECTOR).await?;
    let version = crate::firmware::version_word();
    let mut header = [0xFF; HEADER as usize];
    header[0..4].copy_from_slice(&MAGIC.to_le_bytes());
    header[4..8].copy_from_slice(&sequence.to_le_bytes());
    header[8..12].copy_from_slice(&version.to_le_bytes());
    // Bytes 12..16, the length, stay erased until the sector is closed.
    flash.program(address, &header).await
}

#[cfg(feature = "field-log")]
/// Closes sector `*sector` (which holds `used` bytes, header included) by
/// writing its length, and opens the next one round the ring.
async fn advance(flash: &mut SpiFlash, sector: &mut u32, sequence: &mut u32, used: u32) {
    let address = sector_address(*sector);
    let _ = flash
        .program(address + 12, &(used - HEADER).to_le_bytes())
        .await;
    *sector = log_ring::next(*sector);
    *sequence = sequence.wrapping_add(1);
    // If this fails, leave it; the reader trims at frame boundaries.
    let _ = open(flash, *sector, *sequence).await;
}

#[cfg(feature = "field-log")]
/// Drains the RTT buffer into the flash, forever. Spawn once, in field
/// builds only.
#[embassy_executor::task]
pub async fn task() {
    // Skip messages that don't fit (rather than truncating them, defmt-rtt's
    // default, which would leave a corrupt frame in the stream). Mode 0 is
    // SEGGER's "no block, skip".
    rtt().flags.store(0, Ordering::Relaxed);

    let (mut sector, mut sequence) = {
        let mut flash = FLASH.lock().await;
        let flash = flash.as_mut().expect("set at boot");
        if let Err(e) = flash.wake().await {
            warn!("field log: no SPI flash: {}", e);
            return;
        }
        let (sector, sequence) = match newest(flash).await {
            Some((index, sequence)) => (log_ring::next(index), sequence.wrapping_add(1)),
            None => (0, 0),
        };
        if let Err(e) = open(flash, sector, sequence).await {
            warn!("field log: starting a sector failed: {}", e);
            return;
        }
        flash.sleep().await;
        (sector, sequence)
    };
    let mut used = HEADER;
    CURSOR.lock(|c| c.set(Some((sector, used))));
    info!("field log: writing sector {} (#{})", sector, sequence);

    let mut last_flush = embassy_time::Instant::now();
    loop {
        Timer::after_secs(1).await;
        // Each flush wakes the SPI flash and programs a (partial) page, which
        // costs about the same for 10 bytes as for 256. At debug level the
        // log grows ~10 bytes a second, so flushing every second was ~10×
        // the energy of flushing full pages. Wait for a page's worth, or 30 s
        // (so a reset loses little), whichever comes first; the 1 KiB RTT
        // buffer holds far more than either. Unless [`flush`] asks for it
        // now.
        // Only read here: the flag is cleared once what's waiting is in
        // flash (see the end of the loop), which is what [`flush`] waits
        // for. Clearing it on reading would let `flush` return while this
        // still waits for the flash's lock behind another task.
        let urgent = FLUSH_NOW.load(Ordering::Relaxed);
        // A lap-test build (see `LAP_TEST`) writes a line a second, saying
        // which sector it should land in, and doesn't wait for more.
        if LAP_TEST {
            info!("field log: lap test, sector {} (#{})", sector, sequence);
        }
        if !urgent
            && !LAP_TEST
            && waiting() < spi_flash::PAGE as usize
            && last_flush.elapsed() < embassy_time::Duration::from_secs(30)
        {
            continue;
        }
        // Stay off the flash during an update: the update's page writes,
        // which hold up its Bluetooth traffic, shouldn't wait behind ours
        // (an erase can take 300 ms). What's logged meanwhile is dropped
        // once the RTT buffer fills.
        if !urgent && crate::dfu::PROGRESS.lock(Cell::get).is_some() {
            continue;
        }
        last_flush = embassy_time::Instant::now();
        if frames(usize::MAX).is_none() {
            if urgent {
                FLUSH_NOW.store(false, Ordering::Relaxed);
            }
            continue;
        }
        let mut flash = FLASH.lock().await;
        let flash = flash.as_mut().expect("set at boot");
        if flash.wake().await.is_err() {
            // Nothing more can be written, so `flush` needn't wait out its
            // timeout.
            if urgent {
                FLUSH_NOW.store(false, Ordering::Relaxed);
            }
            continue;
        }
        // Everything waiting, including what's logged while this runs (the
        // erase of a new sector, especially). Whole frames only per sector,
        // so every sector decodes on its own.
        while let Some((first, second)) = frames((SECTOR - used) as usize) {
            if first.is_empty() {
                advance(flash, &mut sector, &mut sequence, used).await;
                used = HEADER;
                continue;
            }
            let address = sector_address(sector) + used;
            let n = first.len() + second.len();
            let programmed = match flash.write(address, first).await {
                Ok(()) if !second.is_empty() => {
                    flash.write(address + first.len() as u32, second).await
                }
                result => result,
            };
            if programmed.is_ok() {
                used += n as u32;
            }
            consume(n);
        }
        // A lap-test build gives each second's lines a sector of their
        // own.
        if LAP_TEST {
            advance(flash, &mut sector, &mut sequence, used).await;
            used = HEADER;
        }
        flash.sleep().await;
        CURSOR.lock(|c| c.set(Some((sector, used))));
        if urgent {
            FLUSH_NOW.store(false, Ordering::Relaxed);
        }
    }
}

/// Whether this is a build for testing the ring on a dev kit (the
/// `field-log-lap` feature): its writer moves on to a new sector every
/// second instead of when one is full, so it goes round the whole ring in
/// a quarter of an hour rather than days, through the step between the
/// ring's two stretches and the wrap from its last sector to its first. A
/// fetch then shows every sector, each holding the line that names it.
/// Never for a watch that's worn: it erases a sector a second.
#[cfg(feature = "field-log")]
const LAP_TEST: bool = cfg!(feature = "field-log-lap");

#[cfg(feature = "field-log")]
/// Set by [`flush`]; the writer clears it once it has flushed.
static FLUSH_NOW: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Writes out everything logged so far, now, rather than within the next
/// 30 s: for [`crate::system_off`], before the watch switches off. Returns
/// once it's in flash (or after a few seconds, if the writer is stuck).
/// Without the field log, there's nothing to do: a probe reads the log.
pub async fn flush() {
    #[cfg(feature = "field-log")]
    {
        FLUSH_NOW.store(true, Ordering::Relaxed);
        // The writer checks once a second, and clears the flag only once
        // everything waiting is in flash.
        for _ in 0..30 {
            Timer::after_millis(100).await;
            if !FLUSH_NOW.load(Ordering::Relaxed) {
                return;
            }
        }
        warn!("field log: flushing timed out");
    }
}

// ==============================================================================
// Reading it back
// ==============================================================================

/// A sector's header, for the fetcher.
pub struct SectorInfo {
    pub sequence: u32,
    /// The image version that wrote it (MCUBoot's `ih_ver` major, minor,
    /// revision, as one little-endian word); see `firmware::version_word`.
    pub version: u32,
    /// Bytes of frames after the header, as far as known: for the sector
    /// being written, what's written so far; for a sector a reset cut off,
    /// all of it (the reader trims).
    pub length: u32,
}

pub async fn sector_info(index: u32) -> Option<SectorInfo> {
    if index >= SECTORS {
        return None;
    }
    let mut flash = FLASH.lock().await;
    let flash = flash.as_mut().expect("set at boot");
    flash.wake().await.ok()?;
    let mut header = [0u8; HEADER as usize];
    flash.read(sector_address(index), &mut header).await;
    flash.sleep().await;
    let word = |i: usize| u32::from_le_bytes(header[i..i + 4].try_into().expect("4 bytes"));
    if word(0) != MAGIC {
        return None;
    }
    let length = match CURSOR.lock(Cell::get) {
        Some((current, used)) if current == index => used - HEADER,
        _ if word(12) == u32::MAX => SECTOR - HEADER,
        _ => word(12).min(SECTOR - HEADER),
    };
    Some(SectorInfo {
        sequence: word(4),
        version: word(8),
        length,
    })
}

/// Reads frame bytes from sector `index`, starting `offset` bytes after its
/// header.
pub async fn read(index: u32, offset: u32, data: &mut [u8]) {
    let mut flash = FLASH.lock().await;
    let flash = flash.as_mut().expect("set at boot");
    if flash.wake().await.is_err() {
        return;
    }
    flash
        .read(sector_address(index) + HEADER + offset, data)
        .await;
    flash.sleep().await;
}

// ==============================================================================
// Reset reasons and panics
// ==============================================================================

/// Logs why the chip last reset, and where the previous run panicked if it
/// did. Call once at boot, after the clock (which logs) is restored.
pub fn report_last_reset() {
    let power = embassy_nrf::pac::POWER;
    let reason = power.resetreas().read().0;
    // The bits accumulate until cleared (writing 1s clears them).
    power
        .resetreas()
        .write_value(embassy_nrf::pac::power::regs::Resetreas(reason));
    // RESETPIN, DOG, SREQ (a soft reset: ours, or the bootloader's), LOCKUP,
    // OFF, LPCOMP, DIF (debug interface), NFC; 0 means power-on.
    info!(
        "reset reason: {=u32:#x} (1 pin, 2 watchdog, 4 soft, 8 lockup; 0 power-on)",
        reason
    );

    // SAFETY: Reserved RAM (see `memory.x`), only touched here (once, before
    // any task runs) and by the panic handler (which never returns).
    let crumb = unsafe { breadcrumb().read_volatile() };
    unsafe { breadcrumb().write_volatile(Breadcrumb::EMPTY) };
    if crumb.version != crate::firmware::version_word() {
        return;
    }
    // How much stack was left: an overflow runs into the retained block
    // and the statics below it, and would explain anything.
    let stack_left = crumb.sp.wrapping_sub(STACK_END) as i32;
    if crumb.magic == WATCHDOG_MAGIC || crumb.magic == FAULT_MAGIC {
        // `file` and `file_len` hold the PC and LR of the code that hung or
        // faulted; look them up with `llvm-addr2line -fe <the ELF>`.
        warn!(
            "the previous run {=str}: pc {=u32:#010x}, lr {=u32:#010x}, {=i32} B of stack left",
            if crumb.magic == FAULT_MAGIC {
                "hard-faulted"
            } else {
                "hung: watchdog"
            },
            crumb.file as u32,
            crumb.file_len,
            stack_left
        );
        if crumb.magic == FAULT_MAGIC {
            report_fault(&crumb.message);
        }
        report_trace(&crumb.trace);
        return;
    }
    if crumb.magic != BREADCRUMB_MAGIC && crumb.magic != PANICKING_MAGIC {
        return;
    }
    if crumb.magic == PANICKING_MAGIC {
        // The handler stopped partway (see `panic`): a fault in it, or a
        // panic within it, before it could finish.
        warn!("the previous run's panic handler didn't finish");
    }
    // The file name points into our own flash; same image (checked above),
    // so it's still there. Our flash is the primary slot from `FLASH`'s
    // origin to the slot's end (see `memory.x`).
    let in_flash = |address: usize| (0x8100..0x7C000).contains(&address);
    let file = if in_flash(crumb.file as usize) && crumb.file_len < 256 {
        // SAFETY: Same image, so this is the `&'static str` the panic
        // handler saw.
        unsafe {
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                crumb.file,
                crumb.file_len as usize,
            ))
        }
    } else {
        "?"
    };
    // The message as the panic handler kept it (see `panic`): truncated on
    // a character boundary, and empty if it was a formatted one. The
    // breadcrumb's checks above make garbage unlikely, but RAM is RAM: keep
    // whatever prefix is valid UTF-8.
    let message = &crumb.message[..(crumb.message_len as usize).min(MESSAGE)];
    let message = core::str::from_utf8(message).unwrap_or_else(|e| {
        core::str::from_utf8(&message[..e.valid_up_to()]).expect("valid up to there")
    });
    let message = if message.is_empty() {
        "(formatted: not kept)"
    } else {
        message
    };
    warn!(
        "the previous run panicked at {}:{}: {} ({=i32} B of stack left)",
        file, crumb.line, message, stack_left
    );
    report_trace(&crumb.trace);
}

/// Logs a HardFault's registers (see [`FAULT_WORDS`]): what kind of fault
/// it was, at which address if the hardware kept it, and r0-r12, which
/// with the disassembly at the PC say what the faulting code was doing.
fn report_fault(message: &[u8; MESSAGE]) {
    let mut words = [0u32; FAULT_WORDS];
    for (word, bytes) in words.iter_mut().zip(message.as_chunks::<4>().0) {
        *word = u32::from_le_bytes(*bytes);
    }
    let [cfsr, hfsr, address, exc_return, xpsr, r @ ..] = words;
    warn!(
        "  fault: {}, address {=u32:#010x} (CFSR {=u32:#010x}, HFSR {=u32:#010x}, EXC_RETURN {=u32:#010x}, xPSR {=u32:#010x})",
        fault::Kind::of(cfsr, hfsr),
        address,
        cfsr,
        hfsr,
        exc_return,
        xpsr
    );
    warn!(
        "  r0-r3 {=u32:#010x} {=u32:#010x} {=u32:#010x} {=u32:#010x}, r4-r7 {=u32:#010x} {=u32:#010x} {=u32:#010x} {=u32:#010x}",
        r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7]
    );
    warn!(
        "  r8-r12 {=u32:#010x} {=u32:#010x} {=u32:#010x} {=u32:#010x} {=u32:#010x}",
        r[8], r[9], r[10], r[11], r[12]
    );
}

/// Logs a breadcrumb's backtrace: the calls whose return addresses check
/// out as such (the watchdog's are recorded unchecked, for lack of time),
/// innermost first, as the address of each call instruction, so that
/// `llvm-addr2line -fipe <the ELF> <address>` names the calling line (with
/// what was inlined into it). Not the return address itself: after a call
/// that never returns (a panic), that's the first byte of the next function.
fn report_trace(trace: &[u32; TRACE]) {
    let text = text();
    // Inside the call: BL's second half, or BLX itself.
    let mut calls = trace
        .iter()
        .filter(|&&word| return_address(word, &text, true))
        .map(|&word| (word & !1) - 2);
    let Some(first) = calls.next() else {
        return;
    };
    warn!("  backtrace (innermost call first): {=u32:#07x}", first);
    for call in calls {
        warn!("  backtrace: {=u32:#07x}", call);
    }
}

/// Where a panic happened (or the watchdog bit), kept across the reset
/// that follows it.
#[repr(C)]
#[derive(Clone, Copy)]
struct Breadcrumb {
    magic: u32,
    version: u32,
    file: *const u8,
    file_len: u32,
    line: u32,
    /// How many bytes of `message` are the panic's message (0 for a
    /// watchdog bite).
    message_len: u32,
    /// The stack pointer when it happened: how close the stack came to its
    /// end (see [`STACK_END`]).
    sp: u32,
    /// The start of the panic's message, if it's a plain string (see
    /// [`panic`]).
    message: [u8; MESSAGE],
    /// A backtrace, of sorts: see [`scan_stack`].
    trace: [u32; TRACE],
}

impl Breadcrumb {
    const EMPTY: Self = Self {
        magic: 0,
        version: 0,
        file: core::ptr::null(),
        file_len: 0,
        line: 0,
        message_len: 0,
        sp: 0,
        message: [0; MESSAGE],
        trace: [0; TRACE],
    };
}

/// Bytes of a panic's message kept in a breadcrumb: enough for most plain
/// ones (`explicit panic`, `SoftDevice Controller ASSERT`, …). A HardFault
/// keeps its registers there instead (see [`FAULT_WORDS`]), which is
/// what sets the size.
const MESSAGE: usize = 4 * FAULT_WORDS;

/// Return addresses kept in a breadcrumb: what's left of its 192 bytes
/// after the message. Plenty are stale (a `defmt::panic!` logs its message
/// first, leaving the logger's return addresses on the stack), so the real
/// call chain needs room alongside them; in the backtraces from real
/// crashes so far, it's been within the first 25, and since a fault's
/// scan no longer starts inside its frame (see [`fault::frame_words`]),
/// two of those stale words are gone. 23 is what fits beside a fault's
/// registers.
const TRACE: usize = 23;

// ==============================================================================
// Backtraces
// ==============================================================================
//
// The firmware is built without frame pointers or unwind tables, so there's
// no exact unwinding. Instead, the classic embedded stand-in: scan the stack
// for words that are return addresses. A call (`BL`, or `BLX` to a
// register) leaves the address of the instruction after it, with the Thumb
// bit set, in LR, which the callee pushes if it calls on. So a stack word
// that's odd, points into our code, and follows a call instruction there is
// almost certainly a frame's return address. The innermost ones name the
// code that panicked (or hung), and their order is the call chain, give or
// take a stale word left by an earlier, deeper call.
//
// This is what identifies a panic raised through `defmt::panic!` or
// `defmt::unwrap!` in a dependency: those all report defmt's own location
// (`__defmt_default_panic`), whose callers only the backtrace shows.

/// Our code's addresses, from cortex-m-rt's linker script.
fn text() -> core::ops::Range<u32> {
    unsafe extern "C" {
        static __stext: u32;
        static __etext: u32;
    }
    // Only the symbols' addresses are taken, never their contents (which
    // needs no `unsafe`).
    (&raw const __stext) as u32..(&raw const __etext) as u32
}

/// Whether `word` could be a return address: Thumb (odd), into our code,
/// and (with `check`) right after a call instruction.
fn return_address(word: u32, text: &core::ops::Range<u32>, check: bool) -> bool {
    let at = word & !1;
    if word & 1 == 0 || !text.contains(&at) || at < text.start + 4 {
        return false;
    }
    if !check {
        return true;
    }
    // SAFETY: `at - 4..at` is inside our code (checked above), which is
    // memory-mapped flash, always readable.
    let half = |address: u32| unsafe { (address as *const u16).read_volatile() };
    let (first, second) = (half(at - 4), half(at - 2));
    // BL: 11110 S imm10, then 11 J1 1 J2 imm11.
    let bl = first & 0xF800 == 0xF000 && second & 0xD000 == 0xD000;
    // BLX Rm: 0100 0111 1 Rm 000.
    let blx = second & 0xFF87 == 0x4780;
    bl || blx
}

/// Up to [`TRACE`] return addresses from the stack, innermost first,
/// looking at most `words` words up from `from` (and not past the top).
/// With `check`, only words after a call instruction (see
/// [`return_address`]); without, any odd word into our code, which is
/// quicker, for when there's no time (see [`watchdog_bite`]).
fn scan_stack(from: *const u32, words: usize, check: bool) -> [u32; TRACE] {
    unsafe extern "C" {
        static _stack_start: u32;
    }
    let top = (&raw const _stack_start) as usize;
    let text = text();
    let mut trace = [0; TRACE];
    let mut found = 0;
    for i in 0..words {
        // SAFETY: `from` is the current stack (or an exception frame on it),
        // and this stops at the stack's top: all RAM of ours.
        let word = unsafe { from.add(i) };
        if word as usize >= top {
            break;
        }
        let value = unsafe { word.read_volatile() };
        if return_address(value, &text, check) {
            trace[found] = value;
            found += 1;
            if found == TRACE {
                break;
            }
        }
    }
    trace
}

/// The number changes with [`Breadcrumb`]'s layout, so that a breadcrumb
/// left by a build with another layout (but the same version number, as
/// two dev builds have) isn't misread. 2: with the panic's message; 3:
/// with the stack pointer; 4: with a fault's registers.
const BREADCRUMB_MAGIC: u32 = u32::from_le_bytes(*b"urP4");
/// A panic whose handler hasn't finished (see [`panic`]): what's recorded so
/// far is still worth reporting.
const PANICKING_MAGIC: u32 = u32::from_le_bytes(*b"urQ4");
/// A watchdog bite, rather than a panic (see [`catch_watchdog`]).
const WATCHDOG_MAGIC: u32 = u32::from_le_bytes(*b"urW4");
/// A HardFault (see [`HardFault`]).
const FAULT_MAGIC: u32 = u32::from_le_bytes(*b"urF4");
/// The lowest address the stack may use: just above the retained block (see
/// `memory.x`), which an overflow would otherwise run into first, then our
/// statics. [`guard_stack`] puts a tripwire at the bottom. Must match
/// `_stack_end` in `memory.x`.
pub const STACK_END: u32 = 0x2000_D500;
/// The size of [`guard_stack`]'s tripwire: the stack's last bytes, which
/// can't be read or written.
pub const STACK_GUARD: u32 = 32;
/// Must match the `RETAINED` region in `memory.x`.
const BREADCRUMB_ADDRESS: usize = 0x2000_D440;
// The retained block from +0x40 to its end (see `memory.x`).
const _: () = assert!(size_of::<Breadcrumb>() <= 256 - 0x40);

fn breadcrumb() -> *mut Breadcrumb {
    BREADCRUMB_ADDRESS as *mut Breadcrumb
}

/// Asks the watchdog to interrupt just before it resets the chip (two
/// 32 kHz cycles before), so [`watchdog_bite`] can note where the CPU was
/// stuck. Call once, after the watchdog is set up.
///
/// The interrupt gets priority 1, above all of ours (2), so it can preempt
/// a hung handler of ours too; priority 1 is otherwise unused (Nordic
/// reserves it, and this only runs at death). A hang inside a critical
/// section (which masks it) still goes unrecorded.
pub fn catch_watchdog() {
    use embassy_nrf::interrupt::{self, InterruptExt, Priority};
    embassy_nrf::pac::WDT
        .intenset()
        .write(|w| w.set_timeout(true));
    interrupt::WDT.set_priority(Priority::P1);
    // SAFETY: `WDT` below is the handler; it only writes the breadcrumb.
    unsafe { interrupt::WDT.enable() };
}

/// The watchdog's interrupt: hands the stack pointer at entry (where the
/// hardware saved the interrupted code's registers) and EXC_RETURN (which
/// says how many it saved) to `watchdog_bite`.
#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn WDT() {
    core::arch::naked_asm!(
        "mrs r0, msp",
        "mov r1, lr",
        "b {bite}",
        bite = sym watchdog_bite
    );
}

/// Records the interrupted code's PC and LR (frame words 6 and 5) for the
/// next boot, with the return addresses on the stack above the frame, then
/// waits for the reset.
///
/// That's ~60 µs away (two 32 kHz cycles), about 3800 cycles: enough to
/// scan 48 words without checking each candidate, which the next boot does
/// instead (see [`report_trace`]).
extern "C" fn watchdog_bite(frame: *const u32, exc_return: u32) -> ! {
    // SAFETY: `frame` is the exception frame the hardware just pushed.
    let (pc, lr, xpsr) = unsafe {
        (
            frame.add(6).read(),
            frame.add(5).read(),
            frame.add(7).read(),
        )
    };
    // The interrupted code's own stack starts after the frame, whose size
    // depends on whether it had a floating-point context.
    // SAFETY: Still on the stack (the scan stops at its top).
    let above = unsafe { frame.add(fault::frame_words(exc_return, xpsr)) };
    let crumb = Breadcrumb {
        magic: WATCHDOG_MAGIC,
        version: crate::firmware::version_word(),
        file: pc as *const u8,
        file_len: lr,
        line: 0,
        message_len: 0,
        sp: above as u32,
        message: [0; MESSAGE],
        trace: scan_stack(above, 48, false),
    };
    // SAFETY: See `report_last_reset`.
    unsafe { breadcrumb().write_volatile(crumb) };
    loop {
        cortex_m::asm::nop();
    }
}

/// Our panic handler. Leaves a breadcrumb for [`report_last_reset`] (which
/// the next boot logs), logs the panic for a probe (not in field builds;
/// see below), and resets, rather than waiting for the watchdog with the
/// watch frozen.
///
/// The breadcrumb matters for field builds: the panic's own log message is
/// in RAM that the reset clears before the field log can save it. So the
/// breadcrumb keeps the message's start too. Except from a `defmt::panic!`
/// (or `defmt::unwrap!`, …): that logs its message, then panics with none
/// (`explicit panic`, at defmt's own location), so only the backtrace
/// tells where it came from.
///
/// It writes the breadcrumb in place, a piece at a time, cheapest first,
/// and guards against running twice. A panic that interrupts code in the
/// middle of logging (the Bluetooth controller's faults come from its
/// interrupts) makes the `defmt::error!` below panic in turn ("defmt logger
/// taken reentrantly"); with interrupts masked, every round of that used
/// more stack, until it overflowed into the breadcrumb and our statics and
/// the watch hung until the watchdog reset it, with nothing recorded. (Two
/// phone updates ended exactly so.) Now the second round finds
/// [`PANICKING_MAGIC`] and resets at once, keeping the first panic's record.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use core::ptr::addr_of_mut;

    cortex_m::interrupt::disable();
    let crumb = breadcrumb();
    // SAFETY (for all accesses to `crumb` below): see `report_last_reset`;
    // fields are written through raw pointers, never as a whole on the
    // stack, which may be nearly gone.
    unsafe {
        let magic = addr_of_mut!((*crumb).magic);
        if magic.read_volatile() == PANICKING_MAGIC {
            // A panic within this handler: the first one's record stands.
            magic.write_volatile(BREADCRUMB_MAGIC);
            cortex_m::peripheral::SCB::sys_reset();
        }
        // A fault in Nordic's libraries names their assert's file and line
        // instead (see `sdc_fault`).
        let nordic = NORDIC_FILE.load(Ordering::Relaxed);
        let (file, line) = if nordic.is_null() {
            info.location()
                .map_or(("?", 0), |location| (location.file(), location.line()))
        } else {
            // `note_assert` took these from a `&'static str`, and stored
            // the pointer last: one that's set has its length.
            let len = NORDIC_FILE_LEN.load(Ordering::Relaxed) as usize;
            let file = core::str::from_utf8_unchecked(core::slice::from_raw_parts(nordic, len));
            (file, NORDIC_LINE.load(Ordering::Relaxed))
        };
        addr_of_mut!((*crumb).version).write_volatile(crate::firmware::version_word());
        addr_of_mut!((*crumb).file).write_volatile(file.as_ptr());
        addr_of_mut!((*crumb).file_len).write_volatile(file.len() as u32);
        addr_of_mut!((*crumb).line).write_volatile(line);
        addr_of_mut!((*crumb).message_len).write_volatile(0);
        addr_of_mut!((*crumb).sp).write_volatile(cortex_m::register::msp::read());
        addr_of_mut!((*crumb).trace).write_volatile([0; TRACE]);
        magic.write_volatile(PANICKING_MAGIC);

        // The whole stack, checked: there's no hurry here. It starts in
        // this handler's own frame, whose return addresses lead to
        // `panic!`'s caller.
        let sp = cortex_m::register::msp::read() as *const u32;
        addr_of_mut!((*crumb).trace).write_volatile(scan_stack(sp, 4096, true));

        // Only a message that's a plain string (`panic!("…")` without
        // arguments, `explicit panic`, …), not a formatted one (`expect`'s,
        // an index out of bounds, …). Formatting it would take `core::fmt`,
        // the one thing here that could panic in turn, and the location and
        // backtrace already say where it was.
        // Cut by length rather than by slicing the `str`, whose check (for
        // a cut inside a character, which `floor_char_boundary` rules out)
        // would bring in 4.8 KB of Unicode tables to format its panic.
        let message = info.message().as_str().unwrap_or("");
        let len = message.floor_char_boundary(MESSAGE);
        addr_of_mut!((*crumb).message)
            .cast::<u8>()
            .copy_from_nonoverlapping(message.as_ptr(), len);
        addr_of_mut!((*crumb).message_len).write_volatile(len as u32);
        magic.write_volatile(BREADCRUMB_MAGIC);
    }
    // Log it, for a probe, which sees it at once. Not in field builds:
    // there it would only reach the RTT buffer, whose RAM the reset clears
    // before the field log could save it, so the next boot's report of the
    // breadcrumb is all that's kept either way.
    #[cfg(not(feature = "field-log"))]
    {
        // Only now, with everything recorded: this is what can panic again
        // (see above), and then the guard can't tell this panic from a
        // fresh one. So mark it in progress again for the duration.
        // SAFETY: As above.
        unsafe { core::ptr::addr_of_mut!((*crumb).magic).write_volatile(PANICKING_MAGIC) };
        // SAFETY: As above; written in full above.
        let (file, line, message) = unsafe {
            let crumb = &*crumb;
            (
                core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                    crumb.file,
                    crumb.file_len as usize,
                )),
                crumb.line,
                core::str::from_utf8_unchecked(&crumb.message[..crumb.message_len as usize]),
            )
        };
        defmt::error!("panicked at {=str}:{=u32}: {=str}", file, line, message);
        // SAFETY: As above.
        unsafe { core::ptr::addr_of_mut!((*crumb).magic).write_volatile(BREADCRUMB_MAGIC) };
    }
    // With a debugger attached, stop where we are so it can look.
    let dhcsr = cortex_m::peripheral::DCB::PTR;
    // SAFETY: Reading DHCSR is side-effect free.
    if unsafe { (*dhcsr).dhcsr.read() } & 1 != 0 {
        cortex_m::asm::bkpt();
    }
    cortex_m::peripheral::SCB::sys_reset()
}

/// A HardFault (a bus or memory fault, an invalid instruction, … and any
/// fault while interrupts are masked, as in the panic handler): hands the
/// faulting code's exception frame, its r4-r11 and EXC_RETURN to
/// [`hard_fault_record`]. Without this, cortex-m-rt's default spins until
/// the watchdog resets, and the watchdog's interrupt can't preempt a
/// HardFault, so nothing would say what happened.
///
/// Our own trampoline, not cortex-m-rt's (`#[exception]`), which passes
/// only the frame: by the time a Rust handler runs, its prologue may have
/// reused r4-r11, which the hardware doesn't save, and which hold what the
/// faulting code was working with (in 0.11.3-rc.1's first field fault, the
/// base register of the load at the PC). Pushed on the main stack, below
/// the frame; a fault on that stack's guard locks up before getting here
/// anyway (see [`guard_stack`]).
#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn HardFault() {
    core::arch::naked_asm!(
        // EXC_RETURN bit 2: which stack the frame is on (nothing here runs
        // on the process stack, but it costs nothing to know).
        "mov r2, lr",
        "tst r2, #4",
        "ite eq",
        "mrseq r0, msp",
        "mrsne r0, psp",
        "push {{r4-r11}}",
        "mov r1, sp",
        "b {record}",
        record = sym hard_fault_record
    );
}

/// Records a HardFault for the next boot, like a watchdog bite, and
/// resets: the faulting PC and LR, the stack's depth, a backtrace, and the
/// registers that say what kind of fault it was and what the code was
/// working with (see [`FAULT_WORDS`]). `frame` is the exception frame,
/// `callee` the r4-r11 that [`HardFault`] pushed, and `exc_return` the
/// handler's entry LR.
///
/// The fault status registers come from the SCB; each fault escalated to
/// a HardFault (see [`fault`]), so they say which one it was.
extern "C" fn hard_fault_record(frame: *const u32, callee: *const u32, exc_return: u32) -> ! {
    // SAFETY: `frame` is the exception frame the hardware pushed, and
    // `callee` the eight words `HardFault` pushed: both on the stack.
    let [r0, r1, r2, r3, r12, lr, pc, xpsr] = unsafe { frame.cast::<[u32; 8]>().read() };
    let [r4, r5, r6, r7, r8, r9, r10, r11] = unsafe { callee.cast::<[u32; 8]>().read() };
    // SAFETY: Reading the SCB's fault status and address registers has no
    // side effects.
    let (cfsr, hfsr, mmfar, bfar) = unsafe {
        let scb = &*cortex_m::peripheral::SCB::PTR;
        (
            scb.cfsr.read(),
            scb.hfsr.read(),
            scb.mmfar.read(),
            scb.bfar.read(),
        )
    };
    let address = fault::address(cfsr, mmfar, bfar).unwrap_or(0);
    let registers: [u32; FAULT_WORDS] = [
        cfsr, hfsr, address, exc_return, xpsr, r0, r1, r2, r3, r4, r5, r6, r7, r8, r9, r10, r11,
        r12,
    ];
    let mut message = [0; MESSAGE];
    for (bytes, word) in message.as_chunks_mut::<4>().0.iter_mut().zip(registers) {
        *bytes = word.to_le_bytes();
    }
    // The faulting code's own stack starts after the frame, whose size
    // depends on whether it had a floating-point context (see
    // `fault::frame_words`).
    // SAFETY: Still on the stack (the scan stops at its top).
    let above = unsafe { frame.add(fault::frame_words(exc_return, xpsr)) };
    let crumb = Breadcrumb {
        magic: FAULT_MAGIC,
        version: crate::firmware::version_word(),
        file: pc as *const u8,
        file_len: lr,
        line: 0,
        message_len: 0,
        sp: above as u32,
        message,
        trace: scan_stack(above, 256, true),
    };
    // SAFETY: See `report_last_reset`.
    unsafe { breadcrumb().write_volatile(crumb) };
    cortex_m::peripheral::SCB::sys_reset()
}

/// What a HardFault keeps in its breadcrumb's `message`, as little-endian
/// words: CFSR, HFSR, the address the fault was at (or 0, if the hardware
/// didn't keep it; see [`fault::address`]), the handler's EXC_RETURN, the
/// stacked xPSR (whose low 9 bits are the exception number, if the
/// faulting code was itself a handler), then r0-r12 as the faulting code had them.
/// Its LR and PC are the breadcrumb's `file_len` and `file`.
const FAULT_WORDS: usize = 18;

/// Makes every bus fault precise, in a field build: one from a store then
/// reports the store itself, and the address it went to, where the write
/// buffer would let the store retire first and report the fault only once
/// the bus rejected it (an `ImpreciseBus` fault; see [`fault::Kind`]), at
/// whatever the CPU had gone on to by then. On a dev kit, a store past
/// RAM's end was reported two instructions on, in another source line,
/// with no address; with this, at the store, with the address.
///
/// 0.11.3-rc.1's one field HardFault had a PC that couldn't have faulted
/// (see `fault::frame_words`), which is just what an imprecise fault
/// looks like; a field build is the one that would catch it again.
///
/// It costs a little speed: each store waits for the bus to take it,
/// about a cycle more to RAM, a few to a peripheral. The CPU is awake
/// well under 1% of the time, so the battery shouldn't notice. Not in the
/// plain build, whose faults are a probe away from a debugger anyway.
#[cfg(feature = "field-log")]
pub fn precise_bus_faults() {
    // ACTLR's DISDEFWBUF: no write buffering for the default memory map,
    // which is all of ours.
    const DISDEFWBUF: u32 = 1 << 1;
    // SAFETY: ACTLR's other bits keep their values; this one only changes
    // when a bus fault is reported, not what any access does.
    unsafe {
        let icb = &*cortex_m::peripheral::ICB::PTR;
        icb.actlr.modify(|value| value | DISDEFWBUF);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
}

/// Makes the stack's last 32 bytes (above [`STACK_END`]) inaccessible
/// through the MPU, so that an overflow faults instead of silently
/// overwriting the retained block and our statics. The fault can't be
/// handled (its exception frame would go on the overflowed stack), so the
/// core locks up and resets: reset reason 8 (lockup) then means a stack
/// overflow, nearly always. Costs 32 bytes of the 11 KiB stack.
pub fn guard_stack() {
    // Region 0: base STACK_END (32-byte aligned), size 32 bytes
    // (RASR.SIZE = log2(32) - 1), no access (AP = 0), never executable (XN),
    // enabled. Everything else keeps the default memory map (PRIVDEFENA).
    const _: () = assert!(STACK_GUARD == 32);
    const SIZE_32: u32 = 4 << 1;
    const XN: u32 = 1 << 28;
    const ENABLE: u32 = 1;
    const _: () = assert!(STACK_END.is_multiple_of(32));
    // SAFETY: Region 0 is ours (nothing else here uses the MPU), and it
    // covers only the stack's last 32 bytes, which nothing else uses.
    unsafe {
        let mpu = &*cortex_m::peripheral::MPU::PTR;
        mpu.rnr.write(0);
        mpu.rbar.write(STACK_END);
        mpu.rasr.write(XN | SIZE_32 | ENABLE);
        // PRIVDEFENA | ENABLE.
        mpu.ctrl.write((1 << 2) | 1);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
}

// ==============================================================================
// Nordic's fault handlers
// ==============================================================================
//
// The SoftDevice Controller and the MPSL report an internal error (one of
// their asserts firing) by calling a handler that the application registers
// with `sdc_init` and `mpsl_init`. It gets a file and line, which identify the
// assert to Nordic; the code is a binary blob, so nothing else says what went
// wrong.
//
// nrf-sdc and nrf-mpsl register handlers that panic with the file and line,
// but through `defmt::panic!` when their `defmt` features are on, as they
// must be here: trouble-host's `defmt` feature needs `nrf_sdc::Error:
// defmt::Format`, and nrf-sdc's turns on nrf-mpsl's. A `defmt::panic!` only
// logs its message, which the reset then loses (see `panic`).
//
// The crates have no way to set other handlers, but they only ever pass theirs
// to `sdc_init` and `mpsl_init`, once each, at startup. So build.rs has the
// linker wrap those two (`--wrap=sdc_init`): calls to `sdc_init` go to
// `__wrap_sdc_init`, below, and `__real_sdc_init` names the original. The
// wrappers pass the originals our handlers instead, which note the file and
// line for the panic handler to keep in the breadcrumb, then `core::panic!`.

/// The SoftDevice Controller's fault handler (see above). Worded like
/// Nordic's own (in the nRF Connect SDK), to search for.
///
/// # Safety
///
/// `file` is null or a NUL-terminated string, as the controller passes.
pub unsafe extern "C" fn sdc_fault(file: *const core::ffi::c_char, line: u32) {
    // SAFETY: Passed on from our caller.
    note_assert(unsafe { assert_file(file) }, line);
    panic!("SoftDevice Controller ASSERT")
}

/// The MPSL's assert handler (see above); like [`sdc_fault`].
///
/// # Safety
///
/// `file` is null or a NUL-terminated string, as the MPSL passes.
unsafe extern "C" fn mpsl_assert(file: *const core::ffi::c_char, line: u32) {
    // SAFETY: Passed on from our caller.
    note_assert(unsafe { assert_file(file) }, line);
    panic!("MPSL ASSERT")
}

/// The file and line of the Nordic assert that fired, for the panic
/// handler to keep in place of the panic's own location: passed this way,
/// rather than formatted into the message, so that the handler needs no
/// `core::fmt`. The file stays where the blob (or `debug`) has it, in our
/// flash, as a panic's location does.
static NORDIC_FILE: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
static NORDIC_FILE_LEN: AtomicU32 = AtomicU32::new(0);
static NORDIC_LINE: AtomicU32 = AtomicU32::new(0);

fn note_assert(file: &'static str, line: u32) {
    // The pointer last: the panic handler takes the length and line to go
    // with a pointer that's set.
    NORDIC_FILE_LEN.store(file.len() as u32, Ordering::Relaxed);
    NORDIC_LINE.store(line, Ordering::Relaxed);
    NORDIC_FILE.store(file.as_ptr().cast_mut(), Ordering::Relaxed);
}

/// The file name a fault handler gets, or `?` if there's none, or it isn't
/// UTF-8.
///
/// # Safety
///
/// `file` is null or a NUL-terminated string that's never freed.
unsafe fn assert_file(file: *const core::ffi::c_char) -> &'static str {
    if file.is_null() {
        return "?";
    }
    // SAFETY: Non-null, so NUL-terminated (our caller's promise).
    unsafe { core::ffi::CStr::from_ptr(file) }
        .to_str()
        .unwrap_or("?")
}

/// What nrf-sdc's `sdc_init` calls reach (see above).
#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_sdc_init(_theirs: nrf_sdc::raw::sdc_fault_handler_t) -> i32 {
    unsafe extern "C" {
        fn __real_sdc_init(fault_handler: nrf_sdc::raw::sdc_fault_handler_t) -> i32;
    }
    // SAFETY: The call nrf-sdc made, with another handler of the same type.
    unsafe { __real_sdc_init(Some(sdc_fault)) }
}

/// What nrf-mpsl's `mpsl_init` calls reach (see above).
#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_mpsl_init(
    clock: *const nrf_mpsl::raw::mpsl_clock_lfclk_cfg_t,
    low_prio_irq: nrf_mpsl::raw::IRQn_Type,
    _theirs: nrf_mpsl::raw::mpsl_assert_handler_t,
) -> i32 {
    unsafe extern "C" {
        fn __real_mpsl_init(
            clock: *const nrf_mpsl::raw::mpsl_clock_lfclk_cfg_t,
            low_prio_irq: nrf_mpsl::raw::IRQn_Type,
            assert_handler: nrf_mpsl::raw::mpsl_assert_handler_t,
        ) -> i32;
    }
    // SAFETY: The call nrf-mpsl made, with another handler of the same type.
    unsafe { __real_mpsl_init(clock, low_prio_irq, Some(mpsl_assert)) }
}
