//! Ten local presets, stored separately from the game's save. No pointers or inventory handles are persisted.
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::arenaloadout::{real_weapon, Loadout};
use super::inventory::HAND_SLOTS;

pub(crate) const SLOT_COUNT: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "PresetFile")]
pub(crate) struct Preset {
    pub equipment: Loadout,
}

/// Read older character-specific presets without restricting which Nightfarer can use their equipment.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetFile {
    equipment: Loadout,
    #[serde(default, rename = "hero")]
    _legacy_hero: Option<u8>,
}

impl From<PresetFile> for Preset {
    fn from(file: PresetFile) -> Self {
        Self {
            equipment: file.equipment,
        }
    }
}

impl Preset {
    pub fn validate(&self) -> Result<(), String> {
        let mut slots = [false; HAND_SLOTS as usize];
        for w in &self.equipment.weapons {
            if w.slot >= HAND_SLOTS || !real_weapon(w.id) || w.id >> 28 != 0 {
                return Err("Invalid weapon slot or item".into());
            }
            if std::mem::replace(&mut slots[w.slot as usize], true) {
                return Err("Duplicate weapon slot".into());
            }
        }
        if self.equipment.goods.len() > 18 {
            return Err("Too many quick items".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for &(id, qty) in &self.equipment.goods {
            if id >> 28 != 4 || qty == 0 || qty > 999 || !ids.insert(id) {
                return Err("Invalid item or quantity".into());
            }
            if (1000..=1025).contains(&(id & 0x0fff_ffff)) {
                return Err("The starting flask is managed by the game".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Store {
    version: u32,
    #[serde(with = "slot_map")]
    pub slots: [Option<Preset>; SLOT_COUNT],
}

mod slot_map {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        slots: &[Option<Preset>; SLOT_COUNT],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let map: std::collections::BTreeMap<_, _> = slots
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.as_ref().map(|p| ((i + 1).to_string(), p)))
            .collect();
        map.serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[Option<Preset>; SLOT_COUNT], D::Error> {
        let map = std::collections::BTreeMap::<String, Preset>::deserialize(deserializer)?;
        let mut slots = std::array::from_fn(|_| None);
        for (key, preset) in map {
            let n: usize = key.parse().map_err(serde::de::Error::custom)?;
            if !(1..=SLOT_COUNT).contains(&n) || key != n.to_string() {
                return Err(serde::de::Error::custom("Slot must be 1 through 10"));
            }
            slots[n - 1] = Some(preset);
        }
        Ok(slots)
    }
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: 1,
            slots: std::array::from_fn(|_| None),
        }
    }
}

impl Store {
    pub fn read(path: &Path) -> Result<Self, String> {
        if std::fs::metadata(path).is_ok_and(|m| m.len() > 128 * 1024) {
            return Err("Loadout file is too large".into());
        }
        match std::fs::read_to_string(path) {
            Ok(text) => Self::decode(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Cannot read saved loadouts: {e}")),
        }
    }

    fn decode(text: &str) -> Result<Self, String> {
        let store: Self = toml::from_str(text).map_err(|e| format!("Invalid loadout file: {e}"))?;
        if store.version != 1 {
            return Err("Unsupported loadout file version".into());
        }
        for preset in store.slots.iter().flatten() {
            preset.validate()?;
        }
        Ok(store)
    }

    /// Stage a complete file, sync it, then atomically replace the previous one. A failed write keeps the old slots.
    pub fn save(&mut self, path: &Path, slot: usize, preset: Preset) -> Result<(), String> {
        preset.validate()?;
        if slot >= SLOT_COUNT {
            return Err("Choose a slot from 1 to 10".into());
        }
        let mut next = self.clone();
        next.slots[slot] = Some(preset);
        let text = toml::to_string_pretty(&next).map_err(|e| e.to_string())?;
        let temp = path.with_extension("toml.tmp");
        let write = || -> Result<(), String> {
            use std::io::Write;
            let mut file = std::fs::File::create(&temp).map_err(|e| e.to_string())?;
            file.write_all(text.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|e| e.to_string())?;
            drop(file);
            use std::os::windows::ffi::OsStrExt;
            use windows::core::PCWSTR;
            use windows::Win32::Storage::FileSystem::{
                MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
            };
            let from: Vec<_> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
            let to: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: both paths are NUL-terminated and live through the synchronous call.
            unsafe {
                MoveFileExW(
                    PCWSTR(from.as_ptr()),
                    PCWSTR(to.as_ptr()),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            }
            .map_err(|e| e.to_string())
        };
        write()?;
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::arenaloadout::Weapon;

    fn preset() -> Preset {
        Preset {
            equipment: Loadout {
                weapons: vec![Weapon {
                    slot: 1,
                    id: 9_001_100,
                    custom: 123,
                    skill: 456,
                }],
                goods: vec![(0x4000_2b06, 3)],
            },
        }
    }

    #[test]
    fn roundtrip_ten_slots_with_empty_slots_and_custom_skills() {
        let mut s = Store::default();
        s.slots[0] = Some(preset());
        s.slots[9] = Some(preset());
        let text = toml::to_string_pretty(&s).unwrap();
        let decoded = Store::decode(&text).unwrap();
        assert_eq!(decoded.slots, s.slots);
        assert!(decoded.slots[1].is_none());
    }

    #[test]
    fn legacy_character_metadata_is_ignored_and_not_written_back() {
        let mut store = Store::default();
        store.slots[0] = Some(preset());
        let generic = toml::to_string_pretty(&store).unwrap();
        assert!(!generic.contains("hero"));
        for hero in [1, 4, 8, 12] {
            let legacy = generic.replace(
                "[slots.1.equipment]",
                &format!("[slots.1]\nhero = {hero}\n\n[slots.1.equipment]"),
            );
            assert!(legacy.contains("hero ="));
            let loaded = Store::decode(&legacy).unwrap();
            assert_eq!(loaded.slots, store.slots);
            assert!(!toml::to_string_pretty(&loaded).unwrap().contains("hero"));
        }
    }

    #[test]
    fn rejects_invalid_equipment_and_versions() {
        let mut p = preset();
        p.equipment.weapons.push(p.equipment.weapons[0]);
        assert!(p.validate().is_err());
        p = preset();
        p.equipment.goods[0].1 = u32::MAX;
        assert!(p.validate().is_err());
        let s = Store {
            version: 2,
            ..Store::default()
        };
        assert!(Store::decode(&toml::to_string(&s).unwrap()).is_err());
    }

    #[test]
    fn failed_write_keeps_previous_slot() {
        let mut s = Store::default();
        assert!(s
            .save(
                Path::new("Z:/nonexistent/nrpvp-test/loadouts.toml"),
                0,
                preset()
            )
            .is_err());
        assert!(s.slots[0].is_none());
    }

    #[test]
    fn atomic_overwrite_survives_restart_and_preserves_other_slots() {
        let path = std::env::temp_dir().join(format!(
            "nrpvp-store-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut store = Store::read(&path).unwrap();
        store.save(&path, 0, preset()).unwrap();
        store.save(&path, 9, preset()).unwrap();
        let mut replacement = preset();
        replacement.equipment.weapons.clear();
        replacement.equipment.goods.clear();
        store.save(&path, 0, replacement.clone()).unwrap();
        let restarted = Store::read(&path).unwrap();
        assert_eq!(restarted.slots[0], Some(replacement));
        assert_eq!(restarted.slots[9], Some(preset()));
        assert!(restarted.slots[1].is_none());
        std::fs::write(&path, "broken file").unwrap();
        assert!(Store::read(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken file");
        std::fs::remove_file(path).unwrap();
    }
}
