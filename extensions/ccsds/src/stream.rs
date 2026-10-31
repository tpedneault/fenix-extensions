//! A stream page's rows: each packet summarized through the MIB, lost
//! frames and skipped bytes, a followed parameter's samples, and how a
//! recording or live source is framed -- as asked, or guessed from its
//! first octets.

use std::sync::Arc;

use fenix_ccsds::field::Check;
use fenix_ccsds::stream::{self, Framing, Item, Splitter};
use fenix_mib::mission::Mission;
use fenix_mib::{DefRef, MibSet};

use crate::stream_page::{Row, Sample};

/// Most rows a page keeps; a live page drops its oldest past this.
pub(crate) const MAX_ROWS: usize = 500_000;

/// A packet's row: what it is, per the MIB, and whether it checks out.
pub(crate) fn row_of(bytes: Vec<u8>, offset: usize, vc: Option<u8>, set: Option<&MibSet>, m: &Mission) -> Row {
    let Some((pkt, sec, root)) = fenix_ccsds::pus::decode(&bytes, &m.profile) else {
        return Row { offset, bad: Some("not a packet".into()), len: bytes.len(), bytes, ..Default::default() };
    };
    let h = pkt.header;
    let bad = root.walk().into_iter().find_map(|(_, f)| match &f.check {
        Some(Check::Bad(why)) => Some(if f.name == "packet error control" { format!("CRC {}, {why}", f.raw) } else { format!("{}: {why}", f.name) }),
        _ => None,
    });
    let time = sec.as_ref().and_then(|s| s.time).map(|t| fenix_ccsds::time::format_utc(fenix_ccsds::time::tai_to_utc(t, &m.profile.leap)));
    let pus = sec.as_ref().map(|s| (s.service, s.subtype));
    let mut spid = None;
    let mut label = match pus {
        Some((s, st)) => fenix_ccsds::pus::subtype_name(s, st).map(str::to_string).unwrap_or_else(|| format!("{s},{st}")),
        None => format!("APID 0x{:03X}", h.apid),
    };
    if let Some(set) = set {
        if h.is_tc {
            if let Some(tc) = fenix_mib::packets::identify_tc(set, &pkt, sec.as_ref(), &m.profile) {
                label = set.get(tc).name.clone();
            }
        } else if let Some(d) = fenix_mib::packets::identify_tm(set, &pkt, sec.as_ref()) {
            let e = set.get(d);
            spid = Some(e.name.clone());
            label = if e.alias.is_empty() { e.description.clone() } else { e.alias.clone() };
        }
    }
    // A verification report starts with the TC's packet ID and sequence control.
    let verifies = match pus {
        Some((1, _)) if !h.is_tc => {
            let head = 6 + fenix_ccsds::pus::tm_header_len(&m.profile);
            bytes.get(head..head + 4).map(|b| {
                let apid = u16::from_be_bytes([b[0], b[1]]) & 0x7FF;
                let seq = u16::from_be_bytes([b[2], b[3]]) & 0x3FFF;
                format!("TC APID 0x{apid:03X} seq {seq}")
            })
        }
        _ => None,
    };
    Row {
        offset,
        time,
        apid: h.apid,
        is_tc: h.is_tc,
        pus,
        seq: h.seq_count,
        len: bytes.len(),
        spid,
        label,
        bad,
        vc,
        idle: h.apid == fenix_ccsds::packet::IDLE_APID,
        verifies,
        bytes,
        unit: None,
        unit_profile: None,
    }
}

/// Rows, lost frames (offset, VC, how many) and skipped bytes (offset,
/// length, why).
pub(crate) type Converted = (Vec<Row>, Vec<(usize, u8, u64)>, Vec<(usize, usize, String)>);

/// Rows, lost frames and skipped bytes out of splitter items.
pub(crate) fn convert(items: Vec<Item>, set: Option<&MibSet>, m: &Mission, framing: Option<&Framing>) -> Converted {
    let mut rows = Vec::new();
    let mut lost = Vec::new();
    let mut junk = Vec::new();
    // The unit the packets that follow a frame came in, and how it was read.
    let mut unit: Option<Arc<Vec<u8>>> = None;
    let profile = match framing {
        Some(Framing::Frames(p) | Framing::Cltus(p)) => Some(p.clone()),
        _ => None,
    };
    for item in items {
        match item {
            Item::Packet { offset, bytes, vc } => {
                let mut row = row_of(bytes, offset, vc, set, m);
                if unit.is_some() {
                    row.unit_profile = profile.clone();
                }
                row.unit = unit.clone();
                rows.push(row);
            }
            Item::Frame { offset, info, lost: n, rs, raw, bad, .. } => {
                let cltu = raw.starts_with(&fenix_ccsds::coding::CLTU_START);
                if n > 0 {
                    lost.push((offset, info.vcid, n));
                }
                if rs == Some(None) {
                    junk.push((offset, 0, "a Reed-Solomon code word was beyond repair".to_string()));
                }
                if let Some(why) = bad {
                    let what = if cltu { format!("CLTU, TC frame {} on VC {}", info.vc_count, info.vcid) } else { format!("frame {} on VC {}", info.vc_count, info.vcid) };
                    junk.push((offset, 0, format!("{what}: {why}")));
                }
                unit = Some(Arc::new(raw));
            }
            Item::Junk { offset, len, why } => junk.push((offset, len, why)),
        }
    }
    (rows, lost, junk)
}

/// The samples of parameter `def` in `rows` from `from` on: calibrated,
/// with their limit state.
pub(crate) fn samples_of(rows: &[Row], from: usize, set: &MibSet, m: &Mission, def: DefRef) -> Vec<Sample> {
    let name = set.get(def).name.clone();
    let carriers: Vec<String> = set.used_by(def).iter().map(|d| set.get(*d).name.clone()).collect();
    let mut samples = Vec::new();
    for (i, r) in rows.iter().enumerate().skip(from) {
        if !r.spid.as_ref().is_some_and(|s| carriers.contains(s)) {
            continue;
        }
        let Some((root, _)) = fenix_mib::packets::decode_packet(Some(set), &r.bytes, &m.profile, m.base) else { continue };
        let Some(f) = root.walk().into_iter().map(|(_, f)| f).find(|f| f.link == Some(fenix_ccsds::Link::Parameter(name.clone()))).cloned() else { continue };
        let number = f.value.split_whitespace().next().and_then(|v| v.parse::<f64>().ok());
        let off = match &f.check {
            Some(Check::Warn(m)) => Some((m.clone(), false)),
            Some(Check::Bad(m)) => Some((m.clone(), true)),
            _ => None,
        };
        let partial = matches!(&f.check, Some(Check::Ok(m)) if m.ends_with(fenix_mib::packets::NOT_HERE));
        samples.push(Sample { row: i, time: r.time.clone(), raw: f.raw.clone(), value: f.value.clone(), number, off, partial });
    }
    samples
}

/// A source's framing (one of `ccsds_host::FRAMINGS`), `None` for
/// `guess`: `packets`, `frames` (the project's profile, with or without
/// an FECF), `cltus`, `records` after `header` octets (`records N` as
/// written by hand).
pub(crate) fn framing_of(text: &str, header: usize, m: &Mission) -> Option<Framing> {
    let t = text.trim().to_ascii_lowercase();
    match t.split_whitespace().collect::<Vec<_>>().as_slice() {
        ["packets"] => Some(Framing::Packets),
        ["frames" | "cadu" | "cadus", more @ ..] => {
            // `frames fecf` / `frames no-fecf`: a link that differs from
            // the project's frames in that one thing.
            let mut p = m.frames.clone();
            for word in more {
                match *word {
                    "fecf" => p.fecf = true,
                    "no-fecf" => p.fecf = false,
                    _ => return None,
                }
            }
            Some(Framing::Frames(p))
        }
        ["cltu"] | ["cltus"] => Some(Framing::Cltus(m.tc.clone())),
        ["records"] => (header > 0).then_some(Framing::Records { header }),
        ["records", n] => n.parse().ok().map(|header| Framing::Records { header }),
        _ => None,
    }
}

/// A live source's splitter. With `guess`, the first octets are held
/// until they show how they're framed -- or 16 KB came without showing
/// it, and they're read as packets.
pub(crate) struct LiveSplitter {
    pub(crate) splitter: Option<Splitter>,
    pending: Vec<u8>,
}

impl LiveSplitter {
    /// Too little to tell anything from; past it, every chunk is tried.
    const ENOUGH: usize = 64;
    const GIVE_UP: usize = 16 * 1024;

    pub(crate) fn new(framing: Option<Framing>) -> Self {
        LiveSplitter { splitter: framing.map(Splitter::new), pending: Vec::new() }
    }

    /// Items out of `bytes`, and the framing when this is when it was
    /// decided.
    pub(crate) fn feed(&mut self, bytes: &[u8], m: &Mission) -> (Vec<Item>, Option<String>) {
        if let Some(s) = &mut self.splitter {
            return (s.feed(bytes), None);
        }
        self.pending.extend_from_slice(bytes);
        if self.pending.len() < Self::ENOUGH {
            return (Vec::new(), None);
        }
        let (framing, why) = match stream::recognize(&self.pending, true) {
            Some((f, why)) => reconciled(f, why, m),
            None if self.pending.len() >= Self::GIVE_UP => (Framing::Packets, "no framing recognized in 16 KB -- reading as packets".into()),
            None => return (Vec::new(), None),
        };
        let label = format!("{} · {why}", framing.label());
        let mut s = Splitter::new(framing);
        let items = s.feed(&std::mem::take(&mut self.pending));
        self.splitter = Some(s);
        (items, Some(label))
    }
}

/// A guess, reconciled with the project's frame profile when it agrees.
pub(crate) fn guessed(head: &[u8], m: &Mission) -> (Framing, String) {
    let (f, why) = stream::guess(head);
    reconciled(f, why, m)
}

/// What was recognized, with the project's settings where they agree.
fn reconciled(f: Framing, why: String, m: &Mission) -> (Framing, String) {
    match f {
        Framing::Frames(g) if g.length == m.frames.length || g.rs_depth == m.frames.rs_depth && g.rs_depth > 0 => {
            (Framing::Frames(FrameProfileExt::merged(&m.frames, &g)), format!("{why}, the project's frames"))
        }
        // The guess can see the randomizer; the project says the rest.
        Framing::Cltus(g) => (Framing::Cltus(fenix_ccsds::frames::FrameProfile { randomized: g.randomized || m.tc.randomized, ..m.tc.clone() }), format!("{why}, the project's TC frames")),
        other => (other, why),
    }
}

struct FrameProfileExt;
impl FrameProfileExt {
    fn merged(project: &fenix_ccsds::frames::FrameProfile, guess: &fenix_ccsds::frames::FrameProfile) -> fenix_ccsds::frames::FrameProfile {
        fenix_ccsds::frames::FrameProfile { asm: guess.asm, randomized: guess.randomized || project.randomized, ..project.clone() }
    }
}

/// How much of a text log is read for its hex lines.
pub(crate) const TEXT_LIMIT: u64 = 64 * 1024 * 1024;

/// Text, not binary: no NULs, and nearly all of it printable.
pub(crate) fn looks_like_text(head: &[u8]) -> bool {
    !head.is_empty() && !head.contains(&0) && head.iter().filter(|b| b.is_ascii_graphic() || b.is_ascii_whitespace() || **b >= 0x80).count() * 100 >= head.len() * 98
}
