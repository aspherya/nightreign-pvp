//! Write support (T-038): the safety rule, the single-value poke, and the verdict on what the game did with a poked clock.

use crate::winmem::Proc;

/// Only ever write into a Nucleus instance (its exe lives under `...\Nucleus\content\...`), never the real game install,
/// unless the operator explicitly passes `--i-know`.
pub fn poke_allowed(exe_path: &str, i_know: bool) -> bool {
    if i_know {
        return true;
    }
    let norm = exe_path.replace('/', "\\").to_ascii_lowercase();
    norm.contains("\\nucleus\\content\\")
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PokeMode {
    Add(f32),
    Set(f32),
}

impl PokeMode {
    pub fn apply(self, old: f32) -> f32 {
        match self {
            PokeMode::Add(a) => old + a,
            PokeMode::Set(v) => v,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PokeCfg {
    pub mode: PokeMode,
    /// seconds since the DOWN before the poke may happen
    pub at_secs: f32,
    /// poke only once the survivor list is this short
    pub max_survivors: usize,
}

#[derive(Clone, Debug)]
pub struct PokeEvent {
    pub addr: u64,
    pub t: f32,
    pub old: f32,
    pub wanted: f32,
    pub readback: f32,
}

/// Read f32 at `addr`, write `mode.apply(old)`, read back. Logs nothing; the caller prints.
pub fn poke_f32(proc: &Proc, addr: u64, mode: PokeMode, t: f32) -> Result<PokeEvent, String> {
    let mut b = [0u8; 4];
    if !proc.read(addr, &mut b) {
        return Err(format!("cannot read {addr:#X}"));
    }
    let old = f32::from_le_bytes(b);
    let wanted = mode.apply(old);
    proc.write(addr, &wanted.to_le_bytes())?;
    let mut r = [0u8; 4];
    if !proc.read(addr, &mut r) {
        return Err(format!("cannot read back {addr:#X}"));
    }
    Ok(PokeEvent {
        addr,
        t,
        old,
        wanted,
        readback: f32::from_le_bytes(r),
    })
}

/// Slope (units per second) of a pre-poke history; 1.0 when it cannot be estimated.
pub fn estimate_slope(hist: &[(f32, f32)]) -> f32 {
    if hist.len() >= 2 {
        let (a, b) = (hist[0], hist[hist.len() - 1]);
        let dt = b.0 - a.0;
        if dt > 0.5 && ((b.1 - a.1) / dt).is_finite() {
            return (b.1 - a.1) / dt;
        }
    }
    1.0
}

#[derive(Clone, Debug, PartialEq)]
pub enum PokeVerdict {
    KeptCounting,
    SnappedBack,
    Other(String),
    NoSamples,
}

/// Did the value keep counting from the poked value, or snap back onto its old trajectory? `after` are (t, value) samples taken
/// after the poke; the last finite one decides.
pub fn classify_poke(ev: &PokeEvent, slope: f32, after: &[(f32, f32)]) -> PokeVerdict {
    let Some(&(t, v)) = after.iter().rev().find(|s| s.1.is_finite()) else {
        return PokeVerdict::NoSamples;
    };
    let dt = t - ev.t;
    let exp_new = ev.readback + slope * dt;
    let exp_old = ev.old + slope * dt;
    let tol = 1.0 + 0.05 * dt.abs();
    let near_new = (v - exp_new).abs() <= tol;
    let near_old = (v - exp_old).abs() <= tol;
    match (near_new, near_old) {
        (true, true) => PokeVerdict::Other("poke too small to tell the trajectories apart".into()),
        (true, false) => PokeVerdict::KeptCounting,
        (false, true) => PokeVerdict::SnappedBack,
        _ => PokeVerdict::Other(format!(
            "last value {v:.3} at +{dt:.1}s fits neither new+slope*dt ({exp_new:.3}) nor old+slope*dt ({exp_old:.3})"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safety_rule() {
        let nuc = r"C:\Users\Leonardo\AppData\Local\Nucleus\content\ELDEN RING NIGHTREIGN\Instance0\Game\nightreign.exe";
        let real = r"Z:\SteamLibrary\steamapps\common\ELDEN RING NIGHTREIGN\Game\nightreign.exe";
        assert!(poke_allowed(nuc, false));
        assert!(poke_allowed(&nuc.to_uppercase(), false));
        assert!(poke_allowed(&nuc.replace('\\', "/"), false));
        assert!(!poke_allowed(real, false));
        assert!(poke_allowed(real, true));
        assert!(!poke_allowed(r"C:\x\Nucleus\contentious\a.exe", false));
        assert!(!poke_allowed("", false));
        assert!(!poke_allowed("<unknown>", false));
    }

    fn ev() -> PokeEvent {
        PokeEvent {
            addr: 0,
            t: 10.0,
            old: 10.0,
            wanted: 15.0,
            readback: 15.0,
        }
    }

    #[test]
    fn verdicts() {
        let e = ev();
        assert_eq!(
            classify_poke(&e, 1.0, &[(11.0, 16.1), (13.0, 18.0)]),
            PokeVerdict::KeptCounting
        );
        assert_eq!(
            classify_poke(&e, 1.0, &[(11.0, 11.0), (13.0, 13.1)]),
            PokeVerdict::SnappedBack
        );
        assert!(matches!(
            classify_poke(&e, 1.0, &[(13.0, 0.0)]),
            PokeVerdict::Other(_)
        ));
        assert_eq!(classify_poke(&e, 1.0, &[]), PokeVerdict::NoSamples);
        assert_eq!(estimate_slope(&[(0.0, 1.0), (4.0, 5.0)]), 1.0);
        assert_eq!(estimate_slope(&[(0.0, 1.0)]), 1.0);
        assert_eq!(PokeMode::Add(5.0).apply(2.0), 7.0);
        assert_eq!(PokeMode::Set(5.0).apply(2.0), 5.0);
    }

    #[test]
    fn poke_own_process() {
        let cell: &'static std::sync::atomic::AtomicU32 = Box::leak(Box::new(
            std::sync::atomic::AtomicU32::new(3.0f32.to_bits()),
        ));
        let addr = cell as *const _ as u64;
        let ro = Proc::open(std::process::id()).unwrap();
        assert!(
            ro.write(addr, &[0; 4]).is_err(),
            "read-only handle must refuse"
        );
        let rw = Proc::open_rw(std::process::id()).unwrap();
        let ev = poke_f32(&rw, addr, PokeMode::Add(2.5), 0.0).unwrap();
        assert_eq!((ev.old, ev.wanted, ev.readback), (3.0, 5.5, 5.5));
        assert_eq!(
            f32::from_bits(cell.load(std::sync::atomic::Ordering::Relaxed)),
            5.5
        );
    }
}
