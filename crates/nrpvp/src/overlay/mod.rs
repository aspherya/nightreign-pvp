//! Developer UI. `shared` is plain data (snapshot, commands, roster rows) that the main loop and the matchmaking glue use in every
//! build; `panel` is the Dear ImGui overlay (hudhook, Direct3D 12 hooks) and exists only in developer builds (`--features dev`, R1).

pub(crate) mod shared;

#[cfg(feature = "dev")]
mod panel;
#[cfg(feature = "dev")]
pub(crate) use panel::spawn;
