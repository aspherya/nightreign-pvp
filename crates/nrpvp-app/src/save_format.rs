//! Nightreign BND4 save container. Format references and fixture provenance: ../assets/README.md.
//! No game memory is touched. Every entry checksum is verified before any edit.
use aes::cipher::{block_padding::NoPadding, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use md5::{Digest, Md5};
use serde::Serialize;

const KEY: [u8; 16] = [
    0x18, 0xf6, 0x32, 0x66, 0x05, 0xbd, 0x17, 0x8a, 0x55, 0x24, 0x52, 0x3a, 0xc0, 0xa0, 0xc6, 0x09,
];
pub const STARTER: &[u8] = include_bytes!("../assets/starter.sl2");

pub fn valid_id(id: u64) -> bool {
    (0x0110000100000000..=0x01100001ffffffff).contains(&id)
}
fn u32_at(b: &[u8], o: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        b.get(o..o + 4)
            .ok_or("Truncated save data.")?
            .try_into()
            .unwrap(),
    ))
}

struct Entry {
    offset: usize,
    iv: [u8; 16],
    data: Vec<u8>,
}
pub struct Save {
    raw: Vec<u8>,
    entries: Vec<Entry>,
}
#[derive(Debug, Serialize)]
pub struct Character {
    pub slot: usize,
    pub name: String,
}

fn name_offset(data: &[u8]) -> Result<usize, String> {
    let mut cursor = 0x14;
    for _ in 0..5120 {
        let tag = u32_at(data, cursor)? & 0xf0000000;
        cursor += match tag {
            0x80000000 => 88,
            0x90000000 => 16,
            0xc0000000 => 80,
            0 | 0xb0000000 => 8,
            _ => return Err("Unsupported inventory layout in this character.".into()),
        };
    }
    let offset = cursor + 0x94;
    if u32_at(data, offset + 0x5b8)? > 3065 {
        return Err("Unsupported character layout.".into());
    }
    Ok(offset)
}

impl Save {
    pub fn parse(raw: &[u8]) -> Result<Self, String> {
        if raw.get(..4) != Some(b"BND4") || u32_at(raw, 12)? != 14 {
            return Err("Unsupported Nightreign save container.".into());
        }
        let mut entries = Vec::new();
        let mut end = 64 + 14 * 32;
        for i in 0..14 {
            let size = u32_at(raw, 64 + i * 32 + 8)? as usize;
            let offset = u32_at(raw, 64 + i * 32 + 16)? as usize;
            if size < 48 || !size.is_multiple_of(16) || offset < end {
                return Err("Invalid save entry bounds.".into());
            }
            end = offset.checked_add(size).ok_or("Invalid save size.")?;
            let encrypted = raw.get(offset..end).ok_or("Truncated save entry.")?;
            let iv: [u8; 16] = encrypted[..16].try_into().unwrap();
            let mut data = encrypted[16..].to_vec();
            cbc::Decryptor::<aes::Aes128>::new(&KEY.into(), &iv.into())
                .decrypt_padded_mut::<NoPadding>(&mut data)
                .map_err(|_| "Save decryption failed.")?;
            let checksum = data.len() - 28;
            if Md5::digest(&data[4..checksum])[..] != data[checksum..checksum + 16] {
                return Err(format!(
                    "Save checksum failed in entry {i}; no changes were made."
                ));
            }
            entries.push(Entry { offset, iv, data });
        }
        let save = Self {
            raw: raw.to_vec(),
            entries,
        };
        if !valid_id(save.owner()) {
            return Err("The save has no valid Steam owner.".into());
        }
        Ok(save)
    }
    pub fn owner(&self) -> u64 {
        u64::from_le_bytes(self.entries[10].data[8..16].try_into().unwrap())
    }
    pub fn rebind(&mut self, id: u64) -> Result<(), String> {
        if !valid_id(id) {
            return Err("Enter a valid 17-digit SteamID64.".into());
        }
        let old = self.owner().to_le_bytes();
        for entry in &mut self.entries {
            let end = entry.data.len() - 28;
            let mut cursor = 4;
            while cursor + 8 <= end {
                if entry.data[cursor..cursor + 8] == old {
                    entry.data[cursor..cursor + 8].copy_from_slice(&id.to_le_bytes());
                    cursor += 8;
                } else {
                    cursor += 1;
                }
            }
        }
        Ok(())
    }
    pub fn characters(&self) -> Vec<Character> {
        self.entries[..10]
            .iter()
            .enumerate()
            .filter_map(|(slot, e)| {
                let offset = name_offset(&e.data).ok()?;
                let units: Vec<u16> = e.data[offset..offset + 32]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .take_while(|c| *c != 0)
                    .collect();
                let name = String::from_utf16(&units).ok()?;
                (!name.is_empty() && !name.chars().any(char::is_control))
                    .then_some(Character { slot, name })
            })
            .collect()
    }
    pub fn rename(&mut self, slot: usize, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.encode_utf16().count() > 16 || name.chars().any(char::is_control)
        {
            return Err(
                "Use 1 to 16 characters (emoji count as two); no control characters.".into(),
            );
        }
        if !self.characters().iter().any(|c| c.slot == slot) {
            return Err("Choose a supported character slot.".into());
        }
        let entry = &mut self.entries[slot];
        let offset = name_offset(&entry.data)?;
        entry.data[offset..offset + 32].fill(0);
        for (i, unit) in name.encode_utf16().enumerate() {
            entry.data[offset + i * 2..offset + i * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        }
        Ok(())
    }
    pub fn encode(mut self) -> Result<Vec<u8>, String> {
        for entry in &mut self.entries {
            let checksum = entry.data.len() - 28;
            let digest = Md5::digest(&entry.data[4..checksum]);
            entry.data[checksum..checksum + 16].copy_from_slice(&digest);
            let len = entry.data.len();
            cbc::Encryptor::<aes::Aes128>::new(&KEY.into(), &entry.iv.into())
                .encrypt_padded_mut::<NoPadding>(&mut entry.data, len)
                .map_err(|_| "Save encryption failed.")?;
            self.raw[entry.offset + 16..entry.offset + 16 + len].copy_from_slice(&entry.data);
        }
        Ok(self.raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_roundtrip_rebind_and_unicode_name() {
        let save = Save::parse(STARTER).unwrap();
        assert_eq!(save.characters()[0].name, "Champion Of Zeus");
        assert_eq!(save.encode().unwrap(), STARTER);
        let mut save = Save::parse(STARTER).unwrap();
        let id = 0x0110000100001234;
        save.rebind(id).unwrap();
        save.rename(0, "João 🗡").unwrap();
        let result = save.encode().unwrap();
        let parsed = Save::parse(&result).unwrap();
        assert_eq!(parsed.owner(), id);
        assert_eq!(parsed.characters()[0].name, "João 🗡");
        assert_eq!(result.len(), STARTER.len());
        let mut rename_only = Save::parse(STARTER).unwrap();
        rename_only.rename(0, "New name").unwrap();
        let renamed = Save::parse(&rename_only.encode().unwrap()).unwrap();
        let original = Save::parse(STARTER).unwrap();
        for index in 1..14 {
            assert_eq!(renamed.entries[index].data, original.entries[index].data);
        }
    }
    #[test]
    fn rejects_corruption_and_bad_names() {
        let mut bytes = STARTER.to_vec();
        bytes[20000] ^= 1;
        assert!(Save::parse(&bytes).is_err());
        let mut save = Save::parse(STARTER).unwrap();
        for bad in ["", "\n", "12345678901234567", "🗡🗡🗡🗡🗡🗡🗡🗡🗡"] {
            assert!(save.rename(0, bad).is_err());
        }
        assert!(save.rename(9, "Empty").is_err());
        assert!(save.rebind(123).is_err());
        assert!(Save::parse(&STARTER[..300]).is_err());
    }
}
