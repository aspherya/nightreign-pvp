"""Shared helpers: load an NR MSB (.msb.dcx) from the 1.03.3 unpack, read names / POINT / PARTS / route chains."""
import ctypes, struct, os, re, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from msb import sections, u16str
from patch_routes import dcx_unpack
MS = r"Z:\Tools\nr-unpacked\1.03.3\map\mapstudio"
_oodle = ctypes.CDLL(r"Z:\SteamLibrary\steamapps\common\ELDEN RING NIGHTREIGN\Game\oo2core_9_win64.dll")

class Msb:
    def __init__(self, fname):
        _, self.b = dcx_unpack(_oodle, open(os.path.join(MS, fname), 'rb').read())
        self.S = dict((n, offs) for n, v, offs in sections(self.b))
    def name(self, o): return u16str(self.b, o + struct.unpack_from('<q', self.b, o)[0])
    def points(self):
        for o in self.S['POINT_PARAM_ST']:
            yield self.name(o), struct.unpack_from('<3f', self.b, o + 0x14)
    def parts(self):
        for o in self.S['PARTS_PARAM_ST']:
            yield self.name(o), struct.unpack_from('<i', self.b, o + 0xC)[0], struct.unpack_from('<3f', self.b, o + 0x20)
    def routes(self):
        pts = self.S['POINT_PARAM_ST']
        for o in self.S['EVENT_PARAM_ST']:
            if struct.unpack_from('<i', self.b, o + 0xC)[0] != 25: continue
            td = o + struct.unpack_from('<q', self.b, o + 0x20)[0]
            idx = []
            for k in range(64):
                v = struct.unpack_from('<h', self.b, td + 0x10 + 2 * k)[0]
                if v < 0: break
                idx.append(v)
            yield self.name(o), [struct.unpack_from('<3f', self.b, pts[k] + 0x14) for k in idx]

def tiles():
    for f in sorted(os.listdir(MS)):
        m = re.fullmatch(r'm60_(4\d)_(3\d)_(\d)(\d)\.msb\.dcx', f)
        if m: yield f, int(m.group(1)), int(m.group(2)), int(m.group(3)), int(m.group(4))
