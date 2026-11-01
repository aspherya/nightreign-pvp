"""S10 hook census: list every place where the RUNNING nightreign.exe differs from the file on disk, and where the change points.

Read-only (PROCESS_QUERY_INFORMATION | PROCESS_VM_READ). Never writes to the game. Use only on a game started through
scripts/launch.ps1, nrpvp_launcher.exe or Nucleus (never the Steam/EAC launch; hard rule 2).

Usage (Python 3.12 with pefile + capstone, see memory note python-disasm-env):
    python hook_census.py [--pid N] [--out census.txt]

What it reports, per changed spot in the exe's code (.text sections) and read-only data (.rdata, where vtables live):
    rva, kind (inline jmp / vtable slot / other), target module+rva (or 'private' memory), and for 'private' targets the
    module the trampoline itself jumps into (one hop). Changes made by the game's own protection (decrypted code) show up
    as 'other' with no jump target; they are counted, not listed one by one.
The output holds addresses and module names only, no code bytes from nrsc.dll (hard rule 3).
"""
import argparse, ctypes, ctypes.wintypes as wt, struct, sys
import pefile, capstone

k32 = ctypes.WinDLL('kernel32', use_last_error=True)
psapi = ctypes.WinDLL('psapi', use_last_error=True)
PROCESS_QUERY_INFORMATION, PROCESS_VM_READ = 0x0400, 0x0010

class MODULEINFO(ctypes.Structure):
    _fields_ = [('lpBaseOfDll', ctypes.c_void_p), ('SizeOfImage', wt.DWORD), ('EntryPoint', ctypes.c_void_p)]

def find_pid():
    import subprocess
    out = subprocess.run(['tasklist', '/FI', 'IMAGENAME eq nightreign.exe', '/FO', 'CSV', '/NH'], capture_output=True, text=True).stdout
    pids = [int(l.split('","')[1]) for l in out.splitlines() if l.startswith('"nightreign.exe"')]
    if len(pids) != 1:
        sys.exit(f'need exactly one nightreign.exe (found {pids}); pass --pid')
    return pids[0]

def modules(h):
    arr = (ctypes.c_void_p * 1024)(); need = wt.DWORD()
    psapi.EnumProcessModulesEx(h, arr, ctypes.sizeof(arr), ctypes.byref(need), 3)
    mods = []
    for i in range(need.value // ctypes.sizeof(ctypes.c_void_p)):
        name = ctypes.create_unicode_buffer(520); mi = MODULEINFO()
        psapi.GetModuleFileNameExW(h, ctypes.c_void_p(arr[i]), name, 520)
        psapi.GetModuleInformation(h, ctypes.c_void_p(arr[i]), ctypes.byref(mi), ctypes.sizeof(mi))
        mods.append((mi.lpBaseOfDll, mi.SizeOfImage, name.value))
    return mods

def read(h, addr, n):
    buf = ctypes.create_string_buffer(n); got = ctypes.c_size_t()
    if not k32.ReadProcessMemory(h, ctypes.c_void_p(addr), buf, n, ctypes.byref(got)):
        return None
    return buf.raw[:got.value]

def where(mods, a):
    for base, size, name in mods:
        if base <= a < base + size:
            return name.split('\\')[-1], a - base
    return 'private', a

def jump_target(code, at):
    """Target of a jmp at the start of `code` (E9 rel32, FF25 [rip], mov rax,imm64; jmp rax), else None."""
    if code[:1] == b'\xe9':
        return at + 5 + struct.unpack_from('<i', code, 1)[0]
    if code[:2] == b'\xff\x25':
        return ('ind', at + 6 + struct.unpack_from('<i', code, 2)[0])
    if code[:2] == b'\x48\xb8' and code[10:12] == b'\xff\xe0':
        return struct.unpack_from('<Q', code, 2)[0]
    return None

def main():
    ap = argparse.ArgumentParser(); ap.add_argument('--pid', type=int); ap.add_argument('--out', default='hook_census.txt')
    ap.add_argument('--exe', default='nightreign.exe', help='module to census (self-test on another process only)')
    a = ap.parse_args(); pid = a.pid or find_pid()
    h = k32.OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, False, pid)
    if not h:
        sys.exit(f'OpenProcess failed: {ctypes.get_last_error()}')
    mods = modules(h)
    exe = next(m for m in mods if m[2].lower().endswith(a.exe.lower()))
    base, path = exe[0], exe[2]
    pe = pefile.PE(path)
    pe.relocate_image(base)  # disk image with the same relocations the loader applied
    disk = pe.get_memory_mapped_image()
    d = pe.OPTIONAL_HEADER.DATA_DIRECTORY[12]  # IMAGE_DIRECTORY_ENTRY_IAT
    iat = (d.VirtualAddress, d.VirtualAddress + d.Size)
    lines, other = [], 0
    for s in pe.sections:
        name = s.Name.rstrip(b'\0').decode()
        if name not in ('.text', '.rdata'):
            continue
        va, size = s.VirtualAddress, s.Misc_VirtualSize
        for off in range(0, size, 0x10000):
            n = min(0x10000, size - off)
            live = read(h, base + va + off, n)
            if live is None:
                continue
            ref = disk[va + off: va + off + n]
            i = 0
            while i < n:
                if live[i] == ref[i]:
                    i += 1; continue
                j = i
                while j < n and live[j] != ref[j]:
                    j += 1
                rva = va + off + i
                if name == '.rdata':
                    for s8 in range((i // 8) * 8, j, 8):
                        slot = va + off + s8
                        if iat[0] <= slot < iat[1] or s8 + 8 > n:
                            continue  # import address table: filled by the loader, not a hook
                        m, r = where(mods, struct.unpack_from('<Q', live, s8)[0])
                        if m.lower() != a.exe.lower():
                            lines.append(f'{slot:#010x} vtable/data slot -> {m}+{r:#x}')
                        else:
                            other += 1
                else:
                    t = jump_target(live[i:i + 16], base + rva)
                    if t is None and i >= 1:  # patch may start one byte earlier than the first differing byte
                        t = jump_target(live[i - 1:i + 15], base + rva - 1)
                    if isinstance(t, tuple):
                        q = read(h, t[1], 8); t = struct.unpack('<Q', q)[0] if q else None
                    if t is None:
                        other += 1
                    else:
                        m, r = where(mods, t)
                        hop = ''
                        if m == 'private':
                            tc = read(h, t, 64) or b''
                            # follow up to 4 instructions of the trampoline looking for a jump/call into a module
                            md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
                            for ins in md.disasm(tc, t):
                                t2 = jump_target(tc[ins.address - t:ins.address - t + 16], ins.address)
                                if isinstance(t2, tuple):
                                    q = read(h, t2[1], 8); t2 = struct.unpack('<Q', q)[0] if q else None
                                if t2:
                                    hop = ' then -> %s+%#x' % where(mods, t2); break
                        lines.append(f'{rva:#010x} inline jmp ({j - i} B changed) -> {m}+{r:#x}{hop}')
                i = j
    named = ('nrsc.dll', 'nrpvp.dll', 'steam_api64.dll', 'steamclient64.dll', 'me3', 'gameoverlayrenderer64.dll')
    hdr = [f'pid {pid} exe base {base:#x} {path}',
           'modules of interest: ' + ', '.join(f'{m[2].split(chr(92))[-1]}@{m[0]:#x}' for m in mods
                                               if any(k in m[2].lower() for k in named)),
           f'{len(lines)} redirects, {other} other changed runs (decrypted code / data written at runtime)']
    open(a.out, 'w', encoding='utf-8').write('\n'.join(hdr + lines) + '\n')
    print('\n'.join(hdr)); print(f'written {a.out}')

if __name__ == '__main__':
    main()
