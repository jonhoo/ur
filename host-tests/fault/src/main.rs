//! Host tests for the firmware's `fault` module. `cargo test` runs them.

#[path = "../../../src/fault.rs"]
#[allow(dead_code)]
mod fault;

fn main() {
    println!("run `cargo test`");
}

#[cfg(test)]
mod tests {
    use super::fault::*;

    /// EXC_RETURN for a return to thread mode on the main stack, with a
    /// floating-point context (what every fault from our tasks has) and
    /// without.
    const THREAD_FP: u32 = 0xFFFF_FFE9;
    const THREAD: u32 = 0xFFFF_FFF9;
    /// To handler mode (a fault in an interrupt handler), with and without.
    const HANDLER_FP: u32 = 0xFFFF_FFE1;
    const HANDLER: u32 = 0xFFFF_FFF1;
    /// A stacked xPSR in Thumb state, without and with the alignment word.
    const XPSR: u32 = 0x0100_0000;
    const XPSR_PADDED: u32 = XPSR | 1 << 9;

    #[test]
    fn frames_are_8_words_or_26_with_the_fpu() {
        assert_eq!(frame_words(THREAD, XPSR), 8);
        assert_eq!(frame_words(HANDLER, XPSR), 8);
        assert_eq!(frame_words(THREAD_FP, XPSR), 26);
        assert_eq!(frame_words(HANDLER_FP, XPSR), 26);
        assert_eq!(frame_words(THREAD, XPSR_PADDED), 9);
        assert_eq!(frame_words(THREAD_FP, XPSR_PADDED), 27);
    }

    /// 0.11.3-rc.1's first HardFault on a sealed watch, in the heart
    /// task's `Ticker` (pc 0x43eb6, just after a call to `Instant::now`
    /// returned to 0x43eb3). Its handler took every frame for 8 words, so
    /// its backtrace began inside the floating-point area the hardware
    /// reserves but lazy stacking leaves unwritten: two stale return
    /// addresses there (0x491e1 in the time driver's `schedule_wake`, from
    /// an earlier poll, and `now`'s own return address, 0x43eb3, at the
    /// area's reserved word), ahead of the real caller (0x2d3f9, the heart
    /// task). Its "10640 B of stack left" was 72 bytes too many.
    ///
    /// This lays the stack out as it was: the frame, then the `Ticker`'s
    /// poll function's own (r8, r4-r7 and its return address). Starting
    /// above the frame as long as it really is, the first return address
    /// is the caller's.
    #[test]
    fn the_scan_starts_above_the_floating_point_area() {
        let mut stack = vec![0u32; 26];
        // r0-r3, r12, LR, PC, xPSR.
        stack[..8].copy_from_slice(&[0, 0, 0, 0, 0, 0x43eb3, 0x43eb6, XPSR]);
        // S0-S15 and FPSCR: stale, from deeper calls before.
        stack[12] = 0x491e1;
        // The reserved word: `now`'s pushed LR, stale since it returned.
        stack[25] = 0x43eb3;
        // The poll function's frame: r8, r4, r5, r6, r7, LR.
        stack.extend([
            0x2000_a530,
            0,
            0x2000_a638,
            0x2000_a640,
            0x2000_ff00,
            0x2d3f9,
        ]);
        let odd = |words: &[u32]| words.iter().copied().find(|word| word & 1 == 1);

        let above = frame_words(THREAD_FP, stack[7]);
        assert_eq!(odd(&stack[above..]), Some(0x2d3f9));
        // As the old handler scanned it.
        assert_eq!(odd(&stack[8..]), Some(0x491e1));
        assert_eq!(4 * (above - 8), 72);
    }

    #[test]
    fn an_imprecise_store_keeps_no_address() {
        // IMPRECISERR, escalated (FORCED).
        let (cfsr, hfsr) = (1 << 10, 1 << 30);
        assert_eq!(Kind::of(cfsr, hfsr), Kind::ImpreciseBus);
        assert_eq!(address(cfsr, 0xE000_ED34, 0x2002_0000), None);
    }

    #[test]
    fn a_precise_bus_fault_keeps_its_address() {
        // PRECISERR | BFARVALID.
        let cfsr = 1 << 9 | 1 << 15;
        assert_eq!(Kind::of(cfsr, 1 << 30), Kind::PreciseBus);
        assert_eq!(address(cfsr, 0, 0x2002_0000), Some(0x2002_0000));
    }

    #[test]
    fn the_stack_guard_is_an_mpu_data_fault() {
        // DACCVIOL | MMARVALID, at the guard (`field_log::STACK_END`).
        let cfsr = 1 << 1 | 1 << 7;
        assert_eq!(Kind::of(cfsr, 1 << 30), Kind::MpuData);
        assert_eq!(address(cfsr, 0x2000_D500, 0), Some(0x2000_D500));
    }

    /// When an exception's entry fails, the PC is the interrupted code's:
    /// that's what the kind should say, whatever else is set.
    #[test]
    fn entry_failures_come_first() {
        let precise = 1 << 9 | 1 << 15;
        assert_eq!(Kind::of(precise, 1 << 1), Kind::VectorTable);
        assert_eq!(Kind::of(precise | 1 << 12, 1 << 30), Kind::Stacking);
        assert_eq!(Kind::of(1 << 4, 1 << 30), Kind::Stacking);
        assert_eq!(Kind::of(1 << 13, 1 << 30), Kind::LazyFloatingPoint);
    }

    #[test]
    fn usage_faults() {
        let forced = 1 << 30;
        assert_eq!(Kind::of(1 << 16, forced), Kind::Undefined);
        assert_eq!(Kind::of(1 << 17, forced), Kind::InvalidState);
        assert_eq!(Kind::of(1 << 18, forced), Kind::InvalidReturn);
        assert_eq!(Kind::of(1 << 19, forced), Kind::NoCoprocessor);
        assert_eq!(Kind::of(1 << 24, forced), Kind::Unaligned);
        assert_eq!(Kind::of(1 << 25, forced), Kind::DivideByZero);
        assert_eq!(Kind::of(0, forced), Kind::Unknown);
        assert_eq!(Kind::of(0, 1 << 31), Kind::Debug);
    }
}

#[cfg(kani)]
mod proofs {
    use super::fault::*;

    /// Whatever EXC_RETURN and xPSR hold, even garbage, the frame is one of
    /// the four sizes the hardware pushes, so the scan above it stays on
    /// the stack's side of the frame.
    #[kani::proof]
    fn a_frame_is_one_of_four_sizes() {
        let words = frame_words(kani::any(), kani::any());
        assert!(matches!(words, 8 | 9 | 26 | 27));
        kani::cover!(words == 9, "a basic frame, realigned");
        kani::cover!(words == 27, "a frame with the FPU's, realigned");
    }

    /// The address is only ever one the hardware said it kept.
    #[kani::proof]
    fn an_address_needs_its_valid_bit() {
        let (cfsr, mmfar, bfar): (u32, u32, u32) = (kani::any(), kani::any(), kani::any());
        let at = address(cfsr, mmfar, bfar);
        match at {
            Some(at) if cfsr & 1 << 15 != 0 => assert_eq!(at, bfar),
            Some(at) => {
                assert!(cfsr & 1 << 7 != 0);
                assert_eq!(at, mmfar);
            }
            None => assert_eq!(cfsr & (1 << 15 | 1 << 7), 0),
        }
        kani::cover!(
            at.is_some() && cfsr & (1 << 15 | 1 << 7) == (1 << 15 | 1 << 7),
            "both valid, the bus fault's taken"
        );
        kani::cover!(at.is_some() && cfsr & 1 << 15 == 0, "the memory fault's");
    }
}
