//! Class names from outside the process via MSVC x64 RTTI.
//!
//! object -> `*(obj)` is the vtable; `vtable-8` holds the pointer to the RTTICompleteObjectLocator (COL). x64 COL layout:
//! +0 signature (u32, 1), +4 offset, +8 cdOffset, +12 pTypeDescriptor (RVA), +16 pClassHierarchyDescriptor (RVA), +20 pSelf (RVA of
//! the COL itself). TypeDescriptor: +0 vtable ptr, +8 spare, +16 mangled name (`.?AVName@Ns@@`, NUL terminated).
//! [SOURCE: MSVC RTTI layout as used by every x64 RTTI dumper; validated here by signature + pSelf, so a bad guess is rejected.]
#![allow(clippy::chunks_exact_to_as_chunks)]

use crate::mem::{read_lenient, read_u32, read_u64, MemSrc};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub base: u64,
    pub size: u64,
}

pub struct Rtti<'a> {
    mem: &'a dyn MemSrc,
    mods: Vec<Module>,
    cache: Mutex<HashMap<u64, Option<String>>>,
}

/// `.?AVName@Inner@Outer@@` -> `Outer::Inner::Name`. Templates / anything unusual stay as the raw mangled string.
pub fn demangle(raw: &str) -> String {
    let inner = raw
        .strip_prefix(".?AV")
        .or_else(|| raw.strip_prefix(".?AU"))
        .and_then(|s| s.strip_suffix("@@"));
    match inner {
        Some(s)
            if !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@') =>
        {
            let parts: Vec<&str> = s.split('@').collect();
            if parts.iter().any(|p| p.is_empty()) {
                return raw.to_string();
            }
            parts.into_iter().rev().collect::<Vec<_>>().join("::")
        }
        _ => raw.to_string(),
    }
}

fn valid_mangled(b: &[u8]) -> Option<String> {
    let n = b.iter().position(|c| *c == 0)?;
    let s = &b[..n];
    if s.len() < 6 || !s.iter().all(|c| (0x21..0x7F).contains(c)) {
        return None;
    }
    let st = std::str::from_utf8(s).ok()?;
    (st.starts_with(".?A") && st.ends_with("@@")).then(|| st.to_string())
}

impl<'a> Rtti<'a> {
    pub fn new(mem: &'a dyn MemSrc, mods: Vec<Module>) -> Rtti<'a> {
        Rtti {
            mem,
            mods,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn module_of(&self, a: u64) -> Option<&Module> {
        self.mods
            .iter()
            .find(|m| a >= m.base && a - m.base < m.size)
    }

    pub fn in_image(&self, a: u64) -> bool {
        self.module_of(a).is_some()
    }

    /// Name of the class whose vtable (address of the first virtual function slot) is `vt`.
    pub fn class_of_vtable(&self, vt: u64) -> Option<String> {
        if vt & 7 != 0 {
            return None;
        }
        if let Ok(c) = self.cache.lock() {
            if let Some(v) = c.get(&vt) {
                return v.clone();
            }
        }
        let r = self.resolve(vt);
        if let Ok(mut c) = self.cache.lock() {
            c.insert(vt, r.clone());
        }
        r
    }

    fn resolve(&self, vt: u64) -> Option<String> {
        let m = self.module_of(vt)?;
        let col = read_u64(self.mem, vt.checked_sub(8)?)?;
        if col & 3 != 0 || col < m.base || col - m.base >= m.size {
            return None;
        }
        if read_u32(self.mem, col)? != 1 {
            return None;
        }
        let td_rva = read_u32(self.mem, col + 12)? as u64;
        let self_rva = read_u32(self.mem, col + 20)? as u64;
        if self_rva != col - m.base || td_rva == 0 || td_rva >= m.size {
            return None;
        }
        let name = read_lenient(self.mem, m.base + td_rva + 16, 160);
        valid_mangled(&name).map(|s| demangle(&s))
    }

    /// Class of the object that starts at `obj` (reads the vtable pointer at `*(obj)`).
    pub fn class_at(&self, obj: u64) -> Option<String> {
        let vt = read_u64(self.mem, obj)?;
        if !self.in_image(vt) {
            return None;
        }
        self.class_of_vtable(vt)
    }

    /// All named-vtable qwords at 8-aligned addresses in `[addr - max_back, addr]`, nearest first: (object start, class).
    pub fn containing(&self, addr: u64, max_back: u64) -> Vec<(u64, String)> {
        let top = addr & !7;
        let lo = top.saturating_sub(max_back);
        let len = (top - lo) as usize + 8;
        let buf = read_lenient(self.mem, lo, len);
        let mut out = Vec::new();
        let mut a = top;
        loop {
            let o = (a - lo) as usize;
            if o + 8 <= buf.len() {
                let q = u64::from_le_bytes(buf[o..o + 8].try_into().unwrap_or([0; 8]));
                if self.in_image(q) {
                    if let Some(n) = self.class_of_vtable(q) {
                        out.push((a, n));
                    }
                }
            }
            if a == lo {
                break;
            }
            a -= 8;
        }
        out
    }
}

/// `0x…4E4 = <CS::SomeClass @0x…230>+0x2B4`
pub fn fmt_in(addr: u64, obj: u64, class: &str) -> String {
    format!("{addr:#X} = <{class} @{obj:#X}>+{:#X}", addr - obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::FakeMem;

    #[test]
    fn demangle_forms() {
        assert_eq!(demangle(".?AVPlayerIns@CS@@"), "CS::PlayerIns");
        assert_eq!(demangle(".?AUFoo@@"), "Foo");
        assert_eq!(demangle(".?AVInner@Mid@Outer@@"), "Outer::Mid::Inner");
        assert_eq!(demangle(".?AV?$Vec@H@std@@"), ".?AV?$Vec@H@std@@");
        assert_eq!(demangle("garbage"), "garbage");
        assert_eq!(demangle(".?AV@@"), ".?AV@@");
    }

    /// A synthetic 1 MB "image": vtable at +0x1000 (slot 0 at +0x1000, COL pointer at +0xFF8), COL at +0x2000, TD at +0x3000.
    fn image() -> (FakeMem, Vec<Module>) {
        let base = 0x1_4000_0000u64;
        let mut m = FakeMem::default();
        m.zeros(base, 0x10000);
        let col = base + 0x2000;
        m.put_u64(base + 0xFF8, col);
        let mut c = vec![0u8; 24];
        c[0..4].copy_from_slice(&1u32.to_le_bytes());
        c[12..16].copy_from_slice(&0x3000u32.to_le_bytes());
        c[20..24].copy_from_slice(&0x2000u32.to_le_bytes());
        m.put(col, &c);
        let mut td = vec![0u8; 16];
        td.extend_from_slice(b".?AVThing@CS@@\0");
        m.put(base + 0x3000, &td);
        // a second vtable whose COL has a wrong pSelf
        let col2 = base + 0x2100;
        m.put_u64(base + 0x1FF8, col2);
        let mut c2 = c.clone();
        c2[20..24].copy_from_slice(&0x2222u32.to_le_bytes());
        m.put(col2, &c2);
        // heap objects
        let heap = 0x7FF0_0000_0000u64;
        m.zeros(heap, 0x8000);
        m.put_u64(heap + 0x100, base + 0x1000); // valid object
        m.put_u64(heap + 0x4000, base + 0x2000); // vtable-looking pointer without a COL
        m.put_u64(heap + 0x5000, base + 0x2000 + 0x100);
        (
            m,
            vec![Module {
                name: "game.exe".into(),
                base,
                size: 0x10000,
            }],
        )
    }

    #[test]
    fn names_from_synthetic_image() {
        let (m, mods) = image();
        let r = Rtti::new(&m, mods);
        let heap = 0x7FF0_0000_0000u64;
        assert_eq!(r.class_at(heap + 0x100).as_deref(), Some("CS::Thing"));
        assert_eq!(r.class_at(heap + 0x4000), None);
        assert_eq!(r.class_at(heap), None);
        // wrong pSelf is rejected
        assert_eq!(r.class_of_vtable(0x1_4000_1000 + 0x1000), None);
    }

    #[test]
    fn containing_walks_back() {
        let (mut m, mods) = image();
        let heap = 0x7FF0_0000_0000u64;
        m.put_u64(heap + 0x60, 0x1_4000_1000); // a second, outer object
        let r = Rtti::new(&m, mods);
        let v = r.containing(heap + 0x100 + 0x2B4, 0x4000);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], (heap + 0x100, "CS::Thing".to_string()));
        assert_eq!(v[1].0, heap + 0x60);
        assert_eq!(
            fmt_in(heap + 0x3B4, heap + 0x100, "CS::Thing"),
            format!(
                "{:#X} = <CS::Thing @{:#X}>+0x2B4",
                heap + 0x3B4,
                heap + 0x100
            )
        );
        assert!(r.containing(heap + 0x100 + 0x2B4, 0x100).is_empty());
    }

    /// The test binary itself is an MSVC image: scan every module of this process for vtables that the resolver can name (the
    /// CRT / vcruntime ship MSVC RTTI, e.g. `std::exception`, `type_info`).
    #[test]
    fn names_real_msvc_rtti_in_own_process() {
        let proc = crate::winmem::Proc::open(std::process::id()).expect("open self");
        let mods = proc.modules();
        assert!(!mods.is_empty());
        let r = Rtti::new(&proc, mods.clone());
        let mut found: Vec<String> = Vec::new();
        'outer: for m in &mods {
            let img = read_lenient(&proc, m.base, m.size.min(64 << 20) as usize);
            for (i, w) in img.chunks_exact(8).enumerate() {
                let q = u64::from_le_bytes(w.try_into().unwrap());
                if q >= m.base && q < m.base + m.size && q & 7 == 0 {
                    if let Some(n) = r.class_of_vtable(q) {
                        let _ = i;
                        if !found.contains(&n) {
                            found.push(n);
                        }
                        if found.len() >= 5 {
                            break 'outer;
                        }
                    }
                }
            }
        }
        println!("own-process RTTI names: {found:?}");
        assert!(
            !found.is_empty(),
            "no MSVC RTTI class found in this process"
        );
    }
}
