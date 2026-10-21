"""Decompress DCX_KRAK files with the game's oo2core_9_win64.dll. Usage: dcx.py <oodle.dll> <file.dcx>... (writes file without .dcx)."""
import ctypes, struct, sys, os
dll = ctypes.WinDLL(sys.argv[1])
f = dll.OodleLZ_Decompress
f.restype = ctypes.c_int64
f.argtypes = [ctypes.c_void_p, ctypes.c_int64, ctypes.c_void_p, ctypes.c_int64] + [ctypes.c_int64]*3 + [ctypes.c_void_p, ctypes.c_int64, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int64, ctypes.c_int64]
ok = 0
for p in sys.argv[2:]:
    b = open(p, 'rb').read()
    assert b[:4] == b'DCX\0', p
    usize, csize = struct.unpack('>II', b[0x1C:0x24])
    i = b.index(b'DCA\0')
    data = b[i + 8:i + 8 + csize]
    out = ctypes.create_string_buffer(usize)
    n = f(data, len(data), out, usize, 0, 0, 0, None, 0, None, None, None, 0, 3)
    if n != usize:
        print('FAIL', p, n, usize); continue
    open(p[:-4], 'wb').write(out.raw); ok += 1
print('ok', ok, 'of', len(sys.argv) - 2)
