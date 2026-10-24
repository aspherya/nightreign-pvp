//! nro-core: the pure half of the orchestrator. No Win32, no files, no processes: the Windows crate (`nro`) gathers facts and runs
//! things; this crate decides (preflight policy, the me3 profile we write, what me3's log must say, the scenario model).
//! Everything here is unit tested without the game.

pub mod me3log;
pub mod pins;
pub mod preflight;
pub mod profile;
pub mod scenario;
