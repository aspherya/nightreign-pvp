//! Training-area ESD / DLL bridge. See tools/loadouts/patch_talk.py for the matching flag protocol.
//! Presets contain equipment only and apply to whichever Nightfarer the player is currently using.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::loadout_store::{Preset, Store, SLOT_COUNT};
use super::{arenaloadout, flags, inventory};

pub(crate) const SAVE: u32 = 10_007_800;
pub(crate) const LOAD: u32 = 10_007_810;
const OCCUPIED: u32 = 10_007_820;
const READY: u32 = 10_007_830;
const DONE: u32 = 10_007_831;
const ERROR: u32 = 10_007_832;
const REFRESH: u32 = 10_007_834;

pub(crate) struct SavedLoadouts {
    store: Option<Result<Store, String>>,
    path: PathBuf,
    published: bool,
    hub_session: Option<usize>,
    allowed: Arc<AtomicBool>,
    pending: Option<(Arc<AtomicU8>, Instant)>,
}

impl Default for SavedLoadouts {
    fn default() -> Self {
        let path = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                crate::sysinfo::dll_directory(crate::dll_handle()).unwrap_or_default()
            })
            .join("NightreignPvP")
            .join("loadouts.toml");
        Self {
            store: None,
            path,
            published: false,
            hub_session: None,
            allowed: Arc::new(AtomicBool::new(false)),
            pending: None,
        }
    }
}

pub(crate) fn available(w: &super::world::World) -> bool {
    w.local.is_some()
        && w.place == Some(super::views::Place::Roundtable)
        && !w.in_expedition()
        && !crate::mm::arena_match()
        && w.mm_phase == crate::mm::PHASE_IDLE
}

impl SavedLoadouts {
    #[cfg(feature = "dev")]
    pub(crate) fn console(&mut self, w: &super::world::World, command: ConsoleCmd) {
        if command == ConsoleCmd::Status {
            tracing::info!(
                "saved_loadouts: status path={} available={} pending={}",
                self.path.display(),
                available(w),
                self.pending.is_some()
            );
            return;
        }
        if !available(w) || self.pending.is_some() {
            tracing::warn!("saved_loadouts: console request refused (hub must be idle)");
            return;
        }
        if command == ConsoleCmd::Sandbox {
            self.path = std::env::temp_dir().join(format!(
                "nrpvp-loadouts-test-{}-{}.toml",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            self.store = Some(Ok(Store::default()));
            self.published = false;
            tracing::info!(
                "saved_loadouts: isolated test store {}",
                self.path.display()
            );
            return;
        }
        if let ConsoleCmd::Request { save, slot } = command {
            let _ = flags::set(w.base, DONE, false);
            let _ = flags::set(w.base, ERROR, false);
            if let Err(e) = self.request(w.base, save, slot) {
                tracing::warn!("saved_loadouts: console: {e}");
            }
        }
    }

    fn publish(&mut self, base: usize) -> Result<(), String> {
        let store = self
            .store
            .as_ref()
            .ok_or("Loadout store not initialized")?
            .as_ref()
            .map_err(Clone::clone)?;
        for (slot, preset) in store.slots.iter().enumerate() {
            flags::set(base, OCCUPIED + slot as u32, preset.is_some())?;
        }
        flags::set(base, REFRESH, false)?;
        flags::set(base, READY, true)?;
        self.published = true;
        Ok(())
    }

    fn request(&mut self, base: usize, save: bool, slot: usize) -> Result<(), String> {
        if self.pending.is_some() {
            return Err("A loadout is already being loaded".into());
        }
        let store = self
            .store
            .as_mut()
            .ok_or("Loadout store not initialized")?
            .as_mut()
            .map_err(|e| e.clone())?;
        if save {
            let preset = Preset {
                equipment: arenaloadout::capture(base)?,
            };
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            store.save(&self.path, slot, preset)?;
            self.published = false;
            flags::set(base, DONE, true)?;
            tracing::info!(
                "saved_loadouts: saved slot {} to {}",
                slot + 1,
                self.path.display()
            );
            return Ok(());
        }
        let preset = store.slots[slot]
            .clone()
            .ok_or("That loadout slot is empty")?;
        preset.validate()?;
        // Check weapon rows before scheduling any destructive inventory operation.
        for w in &preset.equipment.weapons {
            if super::params::row(base, super::params::EQUIP_PARAM_WEAPON, w.id as i32).is_none()
                && super::params::row(base, super::params::EQUIP_PARAM_CUSTOM_WEAPON, w.id as i32)
                    .is_none()
            {
                return Err("A saved weapon is unavailable in this game build".into());
            }
        }
        let pgd = inventory::local_pgd()?;
        let allowed = Arc::clone(&self.allowed);
        let result = Arc::new(AtomicU8::new(0));
        let completed = Arc::clone(&result);
        inventory::queue(
            base,
            "saved_loadout",
            Box::new(move |base| {
                // Never dereference a PGD retained across an area load or a character change.
                let outcome = if completed.load(Ordering::Acquire) == 0
                    && allowed.load(Ordering::Acquire)
                    && inventory::local_pgd() == Ok(pgd)
                {
                    let previous = arenaloadout::capture(base);
                    let restore =
                        inventory::restore_preset(base, pgd, &preset.equipment).and_then(|()| {
                            if arenaloadout::capture(base)? == preset.equipment {
                                Ok(())
                            } else {
                                Err("Restored equipment does not match the saved loadout".into())
                            }
                        });
                    if restore.is_err() {
                        if let Ok(previous) = previous {
                            if let Err(e) = inventory::restore_preset(base, pgd, &previous) {
                                tracing::warn!(
                                    "saved_loadouts: could not restore previous equipment: {e}"
                                );
                            }
                        }
                    }
                    restore
                } else {
                    Err("The player left the hub before the loadout was applied".into())
                };
                match outcome {
                    Ok(()) => {
                        completed.store(1, Ordering::Release);
                    }
                    Err(e) => {
                        tracing::warn!("saved_loadouts: restore failed: {e}");
                        completed.store(2, Ordering::Release);
                    }
                }
            }),
        );
        self.pending = Some((result, Instant::now()));
        Ok(())
    }
}

#[cfg(feature = "dev")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ConsoleCmd {
    Status,
    Sandbox,
    Request { save: bool, slot: usize },
}

#[cfg(feature = "dev")]
pub(crate) fn parse_console(args: &[String]) -> Result<ConsoleCmd, String> {
    match args {
        [] => Ok(ConsoleCmd::Status),
        [mode] if mode == "sandbox" => Ok(ConsoleCmd::Sandbox),
        [mode, slot] if mode == "save" || mode == "load" => {
            let slot: usize = slot.parse().map_err(|_| "Slot must be 1 through 10")?;
            if !(1..=SLOT_COUNT).contains(&slot) {
                return Err("Slot must be 1 through 10".into());
            }
            Ok(ConsoleCmd::Request {
                save: mode == "save",
                slot: slot - 1,
            })
        }
        _ => Err("Use saved_loadouts [sandbox | save|load <slot 1..10>]".into()),
    }
}

impl crate::features::Feature for SavedLoadouts {
    fn name(&self) -> &'static str {
        "saved_loadouts"
    }
    fn phase(&self) -> crate::features::Phase {
        crate::features::Phase::AfterCombat
    }
    fn tick(&mut self, w: &super::world::World, _ctx: &mut crate::features::Ctx) {
        let on = available(w);
        self.allowed.store(on, Ordering::Release);
        if !on {
            if let Some((result, _)) = self.pending.take() {
                result.store(2, Ordering::Release);
            }
            self.published = false;
            self.hub_session = None;
            let _ = flags::set(w.base, READY, false);
            // Requests must not survive departure and execute after returning to the hub.
            for id in SAVE..LOAD + SLOT_COUNT as u32 {
                let _ = flags::set(w.base, id, false);
            }
            return;
        }
        if self.hub_session != Some(w.wcm_instance) {
            // These flags can be present in a game save after an interrupted interaction. Never replay them on launch/return.
            for id in SAVE..LOAD + SLOT_COUNT as u32 {
                let _ = flags::set(w.base, id, false);
            }
            for id in READY..=REFRESH {
                let _ = flags::set(w.base, id, false);
            }
            self.hub_session = Some(w.wcm_instance);
            self.published = false;
        }
        if self.store.is_none() {
            let store = Store::read(&self.path);
            if let Err(e) = &store {
                tracing::warn!("saved_loadouts: {e}; preserving the existing file");
            }
            self.store = Some(store);
        }
        if (!self.published || flags::get(w.base, REFRESH) == Ok(true))
            && self.publish(w.base).is_err()
        {
            let _ = flags::set(w.base, READY, false);
        }
        if let Some((result, since)) = &self.pending {
            let result = result.load(Ordering::Acquire);
            if result != 0 || since.elapsed() > Duration::from_secs(12) {
                if result == 1 {
                    arenaloadout::remember_current(w.base);
                    tracing::info!("saved_loadouts: load completed");
                }
                if result == 0 {
                    self.pending.as_ref().unwrap().0.store(2, Ordering::Release);
                }
                let _ = flags::set(w.base, if result == 1 { DONE } else { ERROR }, true);
                self.pending = None;
            }
        }
        for id in SAVE..LOAD + SLOT_COUNT as u32 {
            if flags::get(w.base, id) != Ok(true) {
                continue;
            }
            let _ = flags::set(w.base, id, false);
            let save = id < LOAD;
            let slot = (id - if save { SAVE } else { LOAD }) as usize;
            if let Err(e) = self.request(w.base, save, slot) {
                tracing::warn!("saved_loadouts: slot {}: {e}", slot + 1);
                super::gametoast::show(&e);
                let _ = flags::set(w.base, ERROR, true);
            }
            break;
        }
    }
}
