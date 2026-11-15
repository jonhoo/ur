//! What a HardFault leaves to go on, worked out from the registers the
//! hardware saves (see `field_log::HardFault`): where the faulting code's
//! stack was, what kind of fault it was, and which address it was at, if
//! the hardware kept it. Pure, so tested on the host (host-tests/fault).
//!
//! Every fault ends in a HardFault here: nothing enables the separate
//! MemManage, BusFault and UsageFault handlers, so each escalates (HFSR's
//! FORCED bit), and the Configurable Fault Status Register (CFSR) says
//! which it was.

// ==============================================================================
// The exception frame
// ==============================================================================

/// Words in the exception frame the hardware pushed for the fault, given
/// the EXC_RETURN value the handler was entered with and the xPSR saved in
/// the frame: the faulting code's own stack starts that many words above
/// the frame.
///
/// 8 for the basic frame (r0-r3, r12, LR, PC, xPSR); 26 when the faulting
/// code had a floating-point context (EXC_RETURN bit 4 clear), whose
/// S0-S15, FPSCR and a reserved word follow. The hardware reserves that
/// space even when lazy stacking leaves it unwritten, so it holds whatever
/// was on the stack before: in 0.11.3-rc.1's first field HardFault, a
/// handler that assumed 8 words reported 72 bytes too much stack left, and
/// started its backtrace with two stale return addresses from there.
/// Thread mode here always has a floating-point context once anything has
/// used the FPU (the executor's sleep reads FPSCR on every loop), so its
/// frames are always the long kind. Then one more word when the hardware
/// aligned the frame to 8 bytes (xPSR bit 9).
pub fn frame_words(exc_return: u32, xpsr: u32) -> usize {
    let basic = 8;
    let floating = if exc_return & EXC_RETURN_BASIC_FRAME == 0 {
        18
    } else {
        0
    };
    let padding = usize::from(xpsr & XPSR_ALIGNED != 0);
    basic + floating + padding
}

/// EXC_RETURN bit 4: set for a basic frame, clear for one with the
/// floating-point registers.
const EXC_RETURN_BASIC_FRAME: u32 = 1 << 4;

/// Stacked xPSR bit 9: the hardware added a word above the frame to align
/// it to 8 bytes.
const XPSR_ALIGNED: u32 = 1 << 9;

// ==============================================================================
// What kind of fault
// ==============================================================================

/// What kind of fault it was, from CFSR and HFSR (the ARMv7-M Architecture
/// Reference Manual's B3.2.15 and B3.2.16). One name for what are often
/// several bits; the registers themselves are logged too.
///
/// Which instruction the stacked PC is matters: for most kinds, it's the
/// one that faulted, but for some it's merely where the code was when the
/// fault arrived, as noted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(target_os = "none", derive(defmt::Format))]
pub enum Kind {
    /// Reading the vector table at an exception's entry failed (HFSR
    /// VECTTBL): the PC is the interrupted code's.
    VectorTable,
    /// A debug event with no debugger to take it (HFSR DEBUGEVT): a BKPT,
    /// say.
    Debug,
    /// Pushing an exception frame failed, on the MPU's guard (MSTKERR) or
    /// the bus (STKERR): the PC is the interrupted code's.
    Stacking,
    /// Popping one at an exception's return failed (MUNSTKERR, UNSTKERR).
    Unstacking,
    /// Lazily saving the floating-point registers failed (MLSPERR, LSPERR).
    LazyFloatingPoint,
    /// A data access the MPU forbids (DACCVIOL): the stack's guard, almost
    /// certainly (see `field_log::guard_stack`). The PC is the access, and
    /// the address recorded (MMARVALID).
    MpuData,
    /// An instruction fetch the MPU forbids (IACCVIOL): from the stack's
    /// guard, which is never executable.
    MpuFetch,
    /// A bus error on a data access, precise (PRECISERR): the PC is the
    /// access, and the address recorded (BFARVALID).
    PreciseBus,
    /// A bus error on a buffered store, reported only after the store
    /// (IMPRECISERR): the PC is some instructions later, possibly in other
    /// code (after an interrupt handler returned, say), and the address
    /// isn't kept.
    ImpreciseBus,
    /// A bus error fetching an instruction (IBUSERR): the PC is the fetch.
    FetchBus,
    /// An undefined instruction (UNDEFINSTR): a jump into data, often.
    Undefined,
    /// Execution in ARM state (INVSTATE): a jump to an even address, as
    /// through a corrupted function pointer or return address.
    InvalidState,
    /// An invalid EXC_RETURN (INVPC): a handler's corrupted return.
    InvalidReturn,
    /// A coprocessor instruction with its coprocessor off (NOCP).
    NoCoprocessor,
    /// An unaligned access that must be aligned (UNALIGNED): LDRD, STRD,
    /// LDM, STM, or an exclusive.
    Unaligned,
    /// A division by zero (DIVBYZERO), if that were trapped (it isn't).
    DivideByZero,
    /// None of the above bits: a HardFault nothing explains.
    Unknown,
}

impl Kind {
    /// The kind `cfsr` and `hfsr` describe. The order puts the faults
    /// that leave the PC elsewhere (the vector table, stacking) first, as
    /// they say the most about what the PC means.
    pub fn of(cfsr: u32, hfsr: u32) -> Self {
        let set = |bit: u32| cfsr & (1 << bit) != 0;
        if hfsr & HFSR_VECTTBL != 0 {
            Self::VectorTable
        } else if hfsr & HFSR_DEBUGEVT != 0 {
            Self::Debug
        } else if set(MSTKERR) || set(STKERR) {
            Self::Stacking
        } else if set(MUNSTKERR) || set(UNSTKERR) {
            Self::Unstacking
        } else if set(MLSPERR) || set(LSPERR) {
            Self::LazyFloatingPoint
        } else if set(DACCVIOL) {
            Self::MpuData
        } else if set(IACCVIOL) {
            Self::MpuFetch
        } else if set(PRECISERR) {
            Self::PreciseBus
        } else if set(IMPRECISERR) {
            Self::ImpreciseBus
        } else if set(IBUSERR) {
            Self::FetchBus
        } else if set(UNDEFINSTR) {
            Self::Undefined
        } else if set(INVSTATE) {
            Self::InvalidState
        } else if set(INVPC) {
            Self::InvalidReturn
        } else if set(NOCP) {
            Self::NoCoprocessor
        } else if set(UNALIGNED) {
            Self::Unaligned
        } else if set(DIVBYZERO) {
            Self::DivideByZero
        } else {
            Self::Unknown
        }
    }
}

/// The address the fault was at, if the hardware kept it: BFAR for a
/// precise bus fault, MMFAR for an MPU fault. (On the Cortex-M4 they're
/// one register, so only one is ever valid.)
pub fn address(cfsr: u32, mmfar: u32, bfar: u32) -> Option<u32> {
    if cfsr & (1 << BFARVALID) != 0 {
        Some(bfar)
    } else if cfsr & (1 << MMARVALID) != 0 {
        Some(mmfar)
    } else {
        None
    }
}

// CFSR's bits: the MemManage status byte (0-7), the BusFault status byte
// (8-15), and the UsageFault status halfword (16-31).
const IACCVIOL: u32 = 0;
const DACCVIOL: u32 = 1;
const MUNSTKERR: u32 = 3;
const MSTKERR: u32 = 4;
const MLSPERR: u32 = 5;
const MMARVALID: u32 = 7;
const IBUSERR: u32 = 8;
const PRECISERR: u32 = 9;
const IMPRECISERR: u32 = 10;
const UNSTKERR: u32 = 11;
const STKERR: u32 = 12;
const LSPERR: u32 = 13;
const BFARVALID: u32 = 15;
const UNDEFINSTR: u32 = 16;
const INVSTATE: u32 = 17;
const INVPC: u32 = 18;
const NOCP: u32 = 19;
const UNALIGNED: u32 = 24;
const DIVBYZERO: u32 = 25;

const HFSR_VECTTBL: u32 = 1 << 1;
const HFSR_DEBUGEVT: u32 = 1 << 31;
