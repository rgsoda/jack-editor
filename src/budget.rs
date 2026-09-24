//! How long a piece of work is allowed to take, in a test.
//!
//! Several tests measure real work - ranking a repository, finding a
//! definition in a large file - and assert it finished inside a bound. The
//! bounds were written on a laptop, and a CI runner is a few times slower
//! than that: slow enough that a debug build there misses a bound that has
//! plenty of room here, while the release run on the same machine is nowhere
//! near it.
//!
//! So the number in the test is the release bound, and a debug build gets it
//! multiplied. That still catches what these tests are for - work that has
//! gone quadratic, or a cache that stopped being used - because those miss by
//! an order of magnitude, not by a fifth.
//!
//! The multiplier is what a shared macOS runner needs rather than what this
//! laptop wants: the same debug test that takes under a second here came in
//! at 2.3 seconds there, which a smaller multiplier fails on a busy morning.

use std::time::Duration;

/// A budget of `millis`, stretched for a debug build.
pub fn budget(millis: u64) -> Duration {
    let slack = if cfg!(debug_assertions) { 8 } else { 1 };
    Duration::from_millis(millis * slack)
}
