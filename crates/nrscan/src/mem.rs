//! Abstract readable memory, so RTTI naming and the route search can run on synthetic images in unit tests.

pub trait MemSrc: Sync {
    /// Read exactly `buf.len()` bytes; false on any failure.
    fn read(&self, addr: u64, buf: &mut [u8]) -> bool;
}

/// Read `len` bytes; unreadable pieces (split on 4 KB page boundaries) stay zero.
pub fn read_lenient(m: &dyn MemSrc, addr: u64, len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    if m.read(addr, &mut buf) {
        return buf;
    }
    let mut off = 0usize;
    while off < len {
        let a = addr + off as u64;
        let n = ((0x1000 - (a & 0xFFF)) as usize).min(len - off);
        let _ = m.read(a, &mut buf[off..off + n]);
        off += n;
    }
    buf
}

pub fn read_u64(m: &dyn MemSrc, addr: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    m.read(addr, &mut b).then(|| u64::from_le_bytes(b))
}

pub fn read_u32(m: &dyn MemSrc, addr: u64) -> Option<u32> {
    let mut b = [0u8; 4];
    m.read(addr, &mut b).then(|| u32::from_le_bytes(b))
}

/// Synthetic memory for tests: a sparse map of byte ranges.
#[cfg(test)]
#[derive(Default)]
pub struct FakeMem {
    pub segs: Vec<(u64, Vec<u8>)>,
}

#[cfg(test)]
impl FakeMem {
    pub fn put(&mut self, addr: u64, bytes: &[u8]) {
        self.segs.push((addr, bytes.to_vec()));
    }
    pub fn put_u64(&mut self, addr: u64, v: u64) {
        self.put(addr, &v.to_le_bytes());
    }
    pub fn zeros(&mut self, addr: u64, len: usize) {
        self.segs.push((addr, vec![0; len]));
    }
}

#[cfg(test)]
impl MemSrc for FakeMem {
    fn read(&self, addr: u64, buf: &mut [u8]) -> bool {
        // Later writes win: compose byte by byte from the segments.
        for (i, b) in buf.iter_mut().enumerate() {
            let a = addr + i as u64;
            let mut found = false;
            for (base, data) in self.segs.iter().rev() {
                if a >= *base && a - *base < data.len() as u64 {
                    *b = data[(a - *base) as usize];
                    found = true;
                    break;
                }
            }
            if !found {
                return false;
            }
        }
        true
    }
}
