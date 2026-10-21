//! Plain data the main loop publishes about the game (`Snapshot`, `PlayerRow`) and the commands it executes (`Command`). The
//! developer console reads and sends these. R5 removed the Dear ImGui panel that used to draw them (the console replaced it).

pub(crate) mod shared;
