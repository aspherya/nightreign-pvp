//! The game's remote TCP endpoints (watchdog L2 measurement), from the system TCP table.

use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
};
use windows_sys::Win32::Networking::WinSock::AF_INET;

pub fn tcp_remotes(pid: u32) -> Vec<String> {
    let mut size = 0u32;
    // SAFETY: first call sizes the buffer, second fills it; rows are read within the reported count.
    unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );
        if size == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u8; size as usize + 1024];
        if GetExtendedTcpTable(
            buf.as_mut_ptr().cast(),
            &mut size,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        ) != 0
        {
            return Vec::new();
        }
        let n = u32::from_ne_bytes(buf[..4].try_into().unwrap()) as usize;
        let rows = buf.as_ptr().add(4) as *const MIB_TCPROW_OWNER_PID;
        let mut out = Vec::new();
        for i in 0..n {
            let r = &*rows.add(i);
            if r.dwOwningPid != pid || r.dwRemoteAddr == 0 {
                continue;
            }
            let a = r.dwRemoteAddr.to_ne_bytes();
            let port = u16::from_be((r.dwRemotePort & 0xFFFF) as u16);
            out.push(format!("{}.{}.{}.{}:{port}", a[0], a[1], a[2], a[3]));
        }
        out
    }
}
