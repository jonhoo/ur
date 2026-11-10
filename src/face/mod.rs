//! Clock faces: the screen the watch comes back to (see `crate::ui`).
//!
//! One is built in, chosen by a Cargo feature: the usual face
//! ([`classic`]) without one, or `face-living-tokyo`'s ([`living_tokyo`]).
//! The others aren't compiled at all. The UI draws whichever it is as
//! [`Active`], through [`ClockFace`].
//!
//! A new face is a module here with a feature of its own (in Cargo.toml,
//! and in `sim/Cargo.toml`, so the simulator can draw it): its `mod` and
//! `Active` below, under that feature, and the feature added to the
//! `classic` lines' `not(any(…))`. Two faces' features on at once would
//! define `Active` twice: give each pair a `compile_error!` saying so.

use crate::alarm;
use crate::canvas::Canvas;
use crate::ui::Status;

#[cfg(not(any(feature = "face-living-tokyo")))]
mod classic;
#[cfg(not(any(feature = "face-living-tokyo")))]
pub type Active = classic::Classic;

#[cfg(feature = "face-living-tokyo")]
mod living_tokyo;
#[cfg(feature = "face-living-tokyo")]
pub type Active = living_tokyo::LivingTokyo;

/// A clock face.
pub trait ClockFace {
    /// Whether the UI marks unseen notifications on this face with a
    /// handle on its top edge (see `ui::edge_bar`). A face that says so
    /// itself, or has its time there, turns it off.
    const UNSEEN_HANDLE: bool = true;

    /// Draws the face, from a clear canvas up.
    fn render(canvas: &mut Canvas, cx: &Context);
}

/// Everything a face may show. Each shows what it likes of it, so a field
/// may go unread in a build with another face.
#[allow(dead_code)]
pub struct Context<'a> {
    pub status: &'a Status<'a>,
    /// The next alarm or timer, and when it rings (local seconds).
    pub next_alarm: Option<(alarm::Kind, i64)>,
    /// `None` without media; otherwise the icon to show for it, if any:
    /// its width, rows, and top.
    pub media: Option<Option<(u32, &'a [u8], i32)>>,
    /// Notifications came in that nobody has seen.
    pub unseen: bool,
    /// Daylight: the light themes are in use (see `ui::App::set_daytime`).
    pub daytime: bool,
    /// The dim look (see `crate::ui`): white and red on black, on a
    /// screen that's barely lit.
    pub dim: bool,
}

/// The day's step goal: it fills the usual face's ten blocks at 1,000 a
/// block, from where they count 10,000 each (see `classic`), and lights
/// every one of Living Tokyo's windows.
///
/// Fixed, because there's nowhere to learn the wearer's own: Gadgetbridge
/// keeps a step goal (its "fitness_goal" setting, 8,000 by default) for its
/// charts, but never sends it to a Bangle.js. Its driver has no message
/// for the user's settings (the call that would send them,
/// `sendSettings`, is commented out in `BangleJSDeviceSupport`), and its
/// `act` message only switches live heart rate and steps on and off.
/// 10,000 is the round number most people know.
///
/// TODO: a goal set on the watch: a tap-to-cycle row on Info, like THEME
/// and LIGHT (say 6k, 8k, 10k, 12k), kept with the settings in flash. The
/// meter's scales ("×1K", "×10K") would then follow the goal.
pub const STEP_GOAL: u32 = 10_000;
