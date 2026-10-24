//! nro: the Windows half of the orchestrator (processes, me3, the mod's console pipe, input, screenshots) and the operations the
//! CLI and the MCP server both expose (`ops`).

pub mod console;
pub mod desk;
pub mod net;
pub mod ops;
pub mod run;
pub mod session;
pub mod sys;
