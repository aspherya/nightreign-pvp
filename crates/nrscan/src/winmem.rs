//! Thin wrapper over the Win32 process APIs. By default the process is opened with PROCESS_QUERY_INFORMATION | PROCESS_VM_READ
//! only; `Proc::open_rw` (T-038, used only by `--poke-*` / `poke` after the Nucleus-path safety check) adds
//! PROCESS_VM_WRITE | PROCESS_VM_OPERATION.

use std::ffi::c_void;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW, Process32NextW,
    MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Memory::{
    VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_PRIVATE, PAGE_EXECUTE_READWRITE,
    PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READWRITE, PAGE_WRITECOPY,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32,
    PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};

pub const MAX_REGION: u64 = 64 << 30; // the game heap holding PlayerIns is one ~1.1 GB region (FS-0, 2026-10-12)

#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub base: u64,
    pub size: u64,
    pub protect: u32,
}

pub struct Proc {
    h: isize,
    pub pid: u32,
    writable: bool,
}

// SAFETY: a process HANDLE is just a kernel object id; ReadProcessMemory/VirtualQueryEx are thread-safe on it.
unsafe impl Send for Proc {}
unsafe impl Sync for Proc {}

impl Proc {
    pub fn open(pid: u32) -> Result<Proc, String> {
        Self::open_with(pid, PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false)
    }

    /// Read + write access. Callers must have passed `poke::poke_allowed` first.
    pub fn open_rw(pid: u32) -> Result<Proc, String> {
        Self::open_with(
            pid,
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION,
            true,
        )
    }

    fn open_with(pid: u32, access: PROCESS_ACCESS_RIGHTS, writable: bool) -> Result<Proc, String> {
        // SAFETY: plain Win32 call.
        let h = unsafe { OpenProcess(access, false, pid) }
            .map_err(|e| format!("OpenProcess({pid}) failed: {e}"))?;
        Ok(Proc {
            h: h.0 as isize,
            pid,
            writable,
        })
    }

    fn handle(&self) -> HANDLE {
        HANDLE(self.h as *mut c_void)
    }

    /// WriteProcessMemory; only on a handle opened with `open_rw`.
    pub fn write(&self, addr: u64, data: &[u8]) -> Result<(), String> {
        if !self.writable {
            return Err("process was opened read-only".into());
        }
        let mut put = 0usize;
        // SAFETY: data is a valid slice; the OS validates the target range (failure is reported, not UB).
        unsafe {
            WriteProcessMemory(
                self.handle(),
                addr as *const c_void,
                data.as_ptr() as *const c_void,
                data.len(),
                Some(&mut put),
            )
        }
        .map_err(|e| format!("WriteProcessMemory({addr:#X}) failed: {e}"))?;
        if put != data.len() {
            return Err(format!("short write: {put} of {} bytes", data.len()));
        }
        Ok(())
    }

    /// Loaded modules (the first entry is the main exe).
    pub fn modules(&self) -> Vec<crate::rtti::Module> {
        let mut out = Vec::new();
        // SAFETY: standard Toolhelp module enumeration on a pid.
        unsafe {
            let Ok(snap) =
                CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, self.pid)
            else {
                return out;
            };
            let mut e = MODULEENTRY32W {
                dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32,
                ..Default::default()
            };
            let mut ok = Module32FirstW(snap, &mut e).is_ok();
            while ok {
                let n = e
                    .szModule
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(e.szModule.len());
                out.push(crate::rtti::Module {
                    name: String::from_utf16_lossy(&e.szModule[..n]),
                    base: e.modBaseAddr as u64,
                    size: e.modBaseSize as u64,
                });
                ok = Module32NextW(snap, &mut e).is_ok();
            }
            let _ = CloseHandle(snap);
        }
        out
    }

    pub fn read(&self, addr: u64, buf: &mut [u8]) -> bool {
        if buf.is_empty() {
            return true;
        }
        let mut got = 0usize;
        // SAFETY: buf is a valid writable slice of buf.len() bytes; the target address is only read in the other process.
        let r = unsafe {
            ReadProcessMemory(
                self.handle(),
                addr as *const c_void,
                buf.as_mut_ptr() as *mut c_void,
                buf.len(),
                Some(&mut got),
            )
        };
        r.is_ok() && got == buf.len()
    }

    /// (base, size, state, type, protect) of the region containing `addr`.
    pub fn query(&self, addr: u64) -> Option<(u64, u64, u32, u32, u32)> {
        let mut mbi = MEMORY_BASIC_INFORMATION::default();
        // SAFETY: mbi is a valid out struct of the size passed.
        let n = unsafe {
            VirtualQueryEx(
                self.handle(),
                Some(addr as *const c_void),
                &mut mbi,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if n == 0 {
            return None;
        }
        Some((
            mbi.BaseAddress as u64,
            mbi.RegionSize as u64,
            mbi.State.0,
            mbi.Type.0,
            mbi.Protect.0,
        ))
    }

    /// Committed, private, read-write regions (no guard / no-access, no images, none larger than 1 GB).
    pub fn regions(&self) -> Vec<Region> {
        let mut out = Vec::new();
        let mut addr = 0u64;
        while addr < 0x7FFF_FFFF_0000 {
            let Some((base, size, state, typ, prot)) = self.query(addr) else {
                break;
            };
            if scannable(state, typ, prot, size) {
                out.push(Region {
                    base,
                    size,
                    protect: prot,
                });
            }
            let next = base.saturating_add(size);
            if next <= addr {
                break;
            }
            addr = next;
        }
        out
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        // SAFETY: we own the handle.
        unsafe {
            let _ = CloseHandle(self.handle());
        }
    }
}

pub fn scannable(state: u32, typ: u32, protect: u32, size: u64) -> bool {
    if state != MEM_COMMIT.0 || typ != MEM_PRIVATE.0 || size == 0 || size > MAX_REGION {
        return false;
    }
    if protect & (PAGE_GUARD.0 | PAGE_NOACCESS.0) != 0 {
        return false;
    }
    protect
        & (PAGE_READWRITE.0
            | PAGE_WRITECOPY.0
            | PAGE_EXECUTE_READWRITE.0
            | PAGE_EXECUTE_WRITECOPY.0)
        != 0
}

pub fn protect_str(p: u32) -> String {
    let base = match p & 0xFF {
        0x01 => "NOACCESS",
        0x02 => "R",
        0x04 => "RW",
        0x08 => "WC",
        0x10 => "X",
        0x20 => "RX",
        0x40 => "RWX",
        0x80 => "XWC",
        _ => "?",
    };
    let mut s = base.to_string();
    if p & PAGE_GUARD.0 != 0 {
        s.push_str("+GUARD");
    }
    s
}

pub struct Game {
    pub pid: u32,
    pub path: String,
}

impl Game {
    pub fn instance(&self) -> Option<String> {
        self.path
            .split(['\\', '/'])
            .find(|c| c.len() > 8 && c[..8].eq_ignore_ascii_case("instance"))
            .map(|s| s.to_string())
    }
}

pub fn exe_path(pid: u32) -> Option<String> {
    let p = Proc::open(pid).ok()?;
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: buf/len describe a valid writable wide-char buffer.
    unsafe {
        QueryFullProcessImageNameW(
            p.handle(),
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    }
    .ok()?;
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

pub fn list_games() -> Vec<Game> {
    let mut out = Vec::new();
    // SAFETY: standard Toolhelp snapshot enumeration.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut e = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let n = e
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(e.szExeFile.len());
            let name = String::from_utf16_lossy(&e.szExeFile[..n]);
            if name.eq_ignore_ascii_case("nightreign.exe") {
                out.push(Game {
                    pid: e.th32ProcessID,
                    path: exe_path(e.th32ProcessID).unwrap_or_else(|| "<unknown>".into()),
                });
            }
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

impl crate::mem::MemSrc for Proc {
    fn read(&self, addr: u64, buf: &mut [u8]) -> bool {
        Proc::read(self, addr, buf)
    }
}
