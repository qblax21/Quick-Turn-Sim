// SPDX-License-Identifier: Apache-2.0
//! The single place in code where the product name is written.
//!
//! Everything that shows a name to a person (window titles, report headers,
//! the generator line in telemetry files) reads it from here. File formats
//! and logic never embed it, so a rename touches this file and the README.

/// Full product name for UI and documents.
pub const PRODUCT_NAME: &str = "Quick Turn Sim";

/// Short product name for compact UI.
pub const PRODUCT_SHORT_NAME: &str = "QTS";

/// Where the optional "Support development" link points (D-120). The game
/// is bought once with every later improvement free: no subscription, no
/// paid add-ons. Donations are optional and unlock nothing. Placeholder
/// until a real page exists; the README links the same address. An empty
/// string hides the link.
pub const DONATE_URL: &str = "https://example.com/quick-turn-sim/support";

/// Version of the simulation core, taken from the crate version.
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default physics rate in ticks per second. Sessions may override it.
pub const DEFAULT_TICK_RATE_HZ: u32 = 1000;

/// Standard gravity in m/s².
pub const GRAVITY_M_S2: f64 = 9.806_65;

/// Identifies the build that produced a replay or hash log.
///
/// Float results are only guaranteed to match between identical builds, so
/// this string is stored in replay headers and compared on load.
pub fn build_id() -> String {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    format!(
        "{}/{}-{}/{}",
        CORE_VERSION,
        std::env::consts::ARCH,
        std::env::consts::OS,
        profile
    )
}
