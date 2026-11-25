//! Reading and writing the section files (.rsp, an XML dialect).

use crate::model::*;

#[derive(Debug, Default, Clone)]
pub struct Elem {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Elem>,
}

impl Elem {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }
    pub fn num(&self, k: &str) -> Option<f64> {
        self.attr(k).and_then(|v| v.trim().parse::<f64>().ok())
    }
    pub fn kids<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Elem> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
}

fn unescape(s: &str) -> String {
    s.replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn parse(src: &str) -> Result<Elem, String> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut stack: Vec<Elem> = vec![Elem { name: "#root".into(), ..Default::default() }];
    while i < b.len() {
        if b[i] == b'<' {
            if src[i..].starts_with("<!--") {
                match src[i..].find("-->") {
                    Some(e) => i += e + 3,
                    None => break,
                }
                continue;
            }
            if src[i..].starts_with("<?") || src[i..].starts_with("<!") {
                match src[i..].find('>') {
                    Some(e) => i += e + 1,
                    None => break,
                }
                continue;
            }
            // find end of tag, honouring quotes
            let mut j = i + 1;
            let mut inq = false;
            while j < b.len() {
                if b[j] == b'"' {
                    inq = !inq;
                } else if b[j] == b'>' && !inq {
                    break;
                }
                j += 1;
            }
            if j >= b.len() {
                return Err("unterminated tag".into());
            }
            let tag = &src[i + 1..j];
            i = j + 1;
            if let Some(name) = tag.strip_prefix('/') {
                let name = name.trim();
                if stack.len() < 2 {
                    return Err(format!("unexpected </{}>", name));
                }
                let e = stack.pop().unwrap();
                if e.name != name {
                    return Err(format!("expected </{}>, found </{}>", e.name, name));
                }
                stack.last_mut().unwrap().children.push(e);
                continue;
            }
            let self_close = tag.trim_end().ends_with('/');
            let tag = tag.trim_end().trim_end_matches('/');
            let mut chars = tag.char_indices().peekable();
            let mut name_end = tag.len();
            while let Some((k, c)) = chars.next() {
                if c.is_whitespace() {
                    name_end = k;
                    break;
                }
            }
            let mut e = Elem { name: tag[..name_end].to_string(), ..Default::default() };
            // attributes
            let rest = &tag[name_end..];
            let rb = rest.as_bytes();
            let mut k = 0;
            while k < rb.len() {
                while k < rb.len() && rb[k].is_ascii_whitespace() {
                    k += 1;
                }
                let s = k;
                while k < rb.len() && rb[k] != b'=' && !rb[k].is_ascii_whitespace() {
                    k += 1;
                }
                let key = rest[s..k].to_string();
                while k < rb.len() && (rb[k].is_ascii_whitespace() || rb[k] == b'=') {
                    k += 1;
                }
                if k < rb.len() && rb[k] == b'"' {
                    k += 1;
                    let vs = k;
                    while k < rb.len() && rb[k] != b'"' {
                        k += 1;
                    }
                    let val = unescape(&rest[vs..k.min(rb.len())]);
                    k += 1;
                    if !key.is_empty() && e.attr(&key).is_none() {
                        e.attrs.push((key, val));
                    }
                } else if key.is_empty() {
                    k += 1;
                }
            }
            if self_close {
                stack.last_mut().unwrap().children.push(e);
            } else {
                stack.push(e);
            }
        } else {
            let e = src[i..].find('<').map(|e| i + e).unwrap_or(b.len());
            stack.last_mut().unwrap().text.push_str(&unescape(&src[i..e]));
            i = e;
        }
    }
    while stack.len() > 1 {
        let e = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(e);
    }
    Ok(stack.pop().unwrap())
}

fn find_all<'a>(e: &'a Elem, name: &str, out: &mut Vec<&'a Elem>) {
    for c in &e.children {
        if c.name == name {
            out.push(c);
        }
        find_all(c, name, out);
    }
}

fn s(e: &Elem, k: &str, d: &str) -> String {
    e.attr(k).map(|v| v.to_string()).unwrap_or_else(|| d.to_string())
}

pub fn load(src: &str) -> Result<Vec<Section>, String> {
    let root = parse(src)?;
    let mut beams = vec![];
    find_all(&root, "r3g_beam", &mut beams);
    if beams.is_empty() {
        return Err("No cross-section found in this file.".into());
    }
    let mut analysis = vec![];
    find_all(&root, "R2010_Analysis", &mut analysis);
    let member = analysis.first().map(|a| {
        let length = a.num("length").unwrap_or(5000.0);
        let mut left = 0.0;
        let mut right = length;
        for sp in a.kids("supports") {
            let p = sp.num("platelocate").unwrap_or(0.0);
            if sp.attr("which") == Some("left") {
                left = p;
            } else {
                right = p;
            }
        }
        Member { length, left_sup: left, right_sup: right, load_at: 0.5 * (left + right) }
    });
    // material factors: <analysisoptions phic phis phip> (the original's reader 0x515ec0, which
    // writes the element only for a factored run), clamped 0.01..10 as its dialog does
    let mut opts = vec![];
    find_all(&root, "analysisoptions", &mut opts);
    let phi = opts
        .first()
        .map(|o| {
            let f = |k: &str| o.num(k).unwrap_or(1.0).clamp(0.01, 10.0);
            [f("phic"), f("phis"), f("phip")]
        })
        .unwrap_or([1.0; 3]);
    // this program's own attributes on the same element: ACI 318 strength reduction factors
    // (design="ACI318"); the original ignores them and reads phic/phis/phip = 1.00
    let aci = opts.first().filter(|o| matches!(o.attr("design"), Some("ACI318") | Some("AASHTO"))).map(|o| {
        let d = if o.attr("design") == Some("AASHTO") { AciFactors::aashto() } else { AciFactors::default() };
        let f = |k: &str, v: f64| o.num(k).filter(|x| *x > 0.0).unwrap_or(v).clamp(0.01, 10.0);
        AciFactors {
            phi_tension: f("phit", d.phi_tension),
            phi_tied: f("phitied", d.phi_tied),
            phi_spiral: f("phispiral", d.phi_spiral),
            spiral: o.num("spiral").map(|v| v > 0.5).unwrap_or(false),
            phi_shear: f("phiv", d.phi_shear),
            ecu: o.num("ecu").filter(|x| *x > 0.0).map(|v| v / 1000.0).unwrap_or(d.ecu),
            tc_extra: d.tc_extra,
            pmax_tied: f("pmaxtied", d.pmax_tied),
            pmax_spiral: f("pmaxspiral", d.pmax_spiral),
            code: d.code,
            phi_tension_ps: f("phitps", d.phi_tension_ps),
            phi_torsion: f("phitor", d.phi_torsion),
        }
    });
    let mut out = vec![];
    for b in beams {
        let mut sec = Section {
            title: s(b, "name", "New Cross Section"),
            done_by: s(b, "doneby", ""),
            date: s(b, "date", ""),
            concretes: vec![],
            rebars: vec![],
            prestress: vec![],
            shape: Shape::Custom,
            outline: vec![],
            long: vec![],
            stirrups: None,
            loads: Loads::new(0.0, 1.0, false),
            member: member.clone(),
            phi,
            aci: aci.clone(),
        };
        for c in b.kids("concrete") {
            let mut k = Concrete::new(&s(c, "name", "Concrete 1"), c.num("fcp").unwrap_or(30.0));
            k.ft = c.num("ft").filter(|v| *v > 0.0);
            k.e0 = c.num("e0").filter(|v| *v > 0.0).map(|v| v.abs());
            k.agg = c.num("maxagg").unwrap_or(19.0);
            k.tsfactor = c.num("tsfactor").unwrap_or(1.0);
            // this program's own extra attribute (the designer's strain limit), if saved
            if let Some(v) = c.num("ecu").filter(|v| *v > 0.0) {
                k.ecu = v;
            }
            k.base = match c.num("c_mod").map(|v| v as i32) {
                Some(1) => BaseCurve::Parabolic,
                _ => BaseCurve::Popovics,
            };
            k.softening = match c.num("c_soft").map(|v| v as i32) {
                Some(0) => Softening::None,
                Some(2) => Softening::VecchioCollins1986,
                _ => Softening::Default2011,
            };
            k.tension_stiffening = c.num("t_stiff").map(|v| v as i32) != Some(0);
            sec.concretes.push(k);
        }
        for r in b.kids("rebar") {
            sec.rebars.push(Rebar {
                name: s(r, "name", "Steel 1"),
                fy: r.num("fy").unwrap_or(400.0),
                fu: r.num("fu").unwrap_or(600.0),
                es: r.num("E").unwrap_or(200000.0),
                esh: r.num("esh").unwrap_or(15.0),
                eu: r.num("eu").unwrap_or(80.0),
                // this program's own extra attribute (the designer's strain limit), if saved
                e_lim: r.num("elim"),
            });
        }
        for p in b.kids("prestress") {
            sec.prestress.push(Prestress {
                name: s(p, "name", "Low Relax"),
                fpu: p.num("fu").unwrap_or(1860.0),
                ep: p.num("E").unwrap_or(200000.0),
                a: p.num("A").unwrap_or(0.025),
                b: p.num("B").unwrap_or(118.0),
                c: p.num("C").unwrap_or(10.0),
                eu: p.num("eu").unwrap_or(43.0),
            });
        }
        if let Some(sol) = b.kids("sectionSOLID").next() {
            let mut pts = vec![];
            for line in sol.text.lines() {
                let nums: Vec<f64> = line
                    .split(|c: char| c.is_whitespace())
                    .filter_map(|t| t.parse::<f64>().ok())
                    .collect();
                if nums.len() >= 2 {
                    pts.push((nums[0], nums[1]));
                }
            }
            // keep file order for steps (same z), but go bottom to top
            if pts.len() >= 2 && pts[0].0 > pts[pts.len() - 1].0 {
                pts.reverse();
            }
            sec.outline = pts;
        }
        if let Some(sh) = b.kids("shapesection").next() {
            let p = |k: &str| sh.num(k).unwrap_or(0.0);
            sec.shape = match sh.attr("name").unwrap_or("") {
                "RECT" => Shape::Rect { b: p("par1"), h: p("par2") },
                "CIRCLE" => Shape::Circle { d: p("par1") },
                "TBEAM" => Shape::Tee { bf: p("par1"), h: p("par2"), bw: p("par3"), hf: p("par4") },
                "IBEAM" => Shape::IBeam { bt: p("par1"), h: p("par2"), bw: p("par3"), ht: p("par4"), hb: p("par5"), bb: p("par6") },
                _ => Shape::Custom,
            };
        }
        if sec.outline.len() < 2 {
            sec.outline = sec.shape.outline();
        }
        for l in b.kids("longreinfx") {
            sec.long.push(LongLayer {
                // the original's defaults (its DTD): name "bot", one bar, A 1
                name: s(l, "name", "bot"),
                z: l.num("z").unwrap_or(0.0),
                num: l.num("num").unwrap_or(1.0).max(1.0) as u32,
                area: l.num("A").unwrap_or(1.0),
                steel: s(l, "type", ""),
                bartitle: s(l, "bartitle", ""),
                dep: l.num("dep").unwrap_or(0.0), ring: if l.num("pattern").unwrap_or(0.0) as u32 == 1 { l.num("nround").unwrap_or(0.0) as u32 } else { 0 }, ring_id: l.num("index").unwrap_or(0.0) as u32,
                // this program's own attribute: the layer's height is given from the top
                from_top: l.num("fromtop").map(|v| v > 0.5).unwrap_or(false),
            });
        }
        // several stirrup sets are merged into one equivalent set (same area per length)
        let sets: Vec<&Elem> = b.kids("transreinfz").collect();
        if let Some(t) = sets.first() {
            let spacing = sets
                .iter()
                .map(|e| e.num("space").unwrap_or(300.0))
                .fold(f64::INFINITY, f64::min)
                .max(1.0);
            let per_len: f64 = sets
                .iter()
                .map(|e| e.num("A").unwrap_or(100.0) / e.num("space").unwrap_or(300.0).max(1.0))
                .sum();
            let area = per_len * spacing;
            let z_top = sets.iter().map(|e| e.num("disttop").unwrap_or(0.0)).fold(f64::MIN, f64::max);
            let z_bot = sets.iter().map(|e| e.num("distbot").unwrap_or(0.0)).fold(f64::MAX, f64::min);
            sec.stirrups = Some(Stirrups {
                name: s(t, "name", "stirrups"),
                area,
                steel: s(t, "type", ""),
                spacing,
                z_top,
                z_bot,
                db: t
                    .num("db")
                    .unwrap_or_else(|| (2.0 * t.num("A").unwrap_or(100.0) / std::f64::consts::PI).sqrt()),
                bartitle: s(t, "bartitle", ""),
                pattern: t.num("pattern").unwrap_or(2.0) as u32,
            });
        }
        if let Some(l) = b.kids("sectloading").next() {
            // base loads BN/BM/BV and load increments MN/MM/MV (kN, kNm)
            // the original's defaults (its DTD): BN = BM = BV = 0, MN = 0, MM = 1, MV = 0
            sec.loads.axial = l.num("BN").or_else(|| l.num("N0")).unwrap_or(0.0);
            sec.loads.m_const = l.num("BM").unwrap_or(0.0);
            sec.loads.v_const = l.num("BV").unwrap_or(0.0);
            sec.loads.n_inc = l.num("MN").unwrap_or(0.0);
            sec.loads.m_inc = l.num("MM").unwrap_or(1.0);
            sec.loads.v_inc = l.num("MV").unwrap_or(0.0);
            sec.loads.sync();
        }
        if sec.concretes.is_empty() {
            sec.concretes.push(Concrete::new("Concrete 1", 30.0));
        }
        if sec.rebars.is_empty() {
            sec.rebars.push(Rebar::new("Steel 1", 400.0));
        }
        out.push(sec);
    }
    Ok(out)
}

pub fn save(sec: &Section) -> String {
    let mut o = String::new();
    o.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\r\n");
    o.push_str("<!DOCTYPE suite_3g SYSTEM \"r3g.dtd\">\r\n\r\n<document>\r\n");
    o.push_str(&format!(
        "\t<r3g_beam\tname\t\t=\"{}\"\r\n\t\t\tdoneby\t=\"{}\"\r\n\t\t\tdate\t\t=\"{}\"\t>\r\n",
        escape(&sec.title),
        escape(&sec.done_by),
        escape(&sec.date)
    ));
    for c in &sec.concretes {
        o.push_str(&format!("\t\t<concrete\tfcp\t\t=\"{:.1}\"\r\n", c.fc));
        if let Some(ft) = c.ft {
            o.push_str(&format!("\t\t\t\tft\t\t=\"{:.2}\"\r\n", ft));
        }
        if let Some(e0) = c.e0 {
            o.push_str(&format!("\t\t\t\te0\t\t=\"{:.3}\"\r\n", e0));
        }
        o.push_str(&format!("\t\t\t\tmaxagg\t=\"{:.1}\"\r\n", c.agg));
        if (c.tsfactor - 1.0).abs() > 1e-9 {
            o.push_str(&format!("\t\t\t\ttsfactor\t=\"{:.2}\"\r\n", c.tsfactor));
        }
        o.push_str(&format!(
            "\t\t\t\tc_mod\t\t=\"{}\"\r\n\t\t\t\tc_soft\t=\"{}\"\r\n\t\t\t\tt_stiff\t=\"{}\"\r\n",
            if c.base == BaseCurve::Parabolic { 1 } else { 2 },
            match c.softening {
                Softening::None => 0,
                Softening::VecchioCollins1986 => 2,
                Softening::Default2011 => 18,
            },
            if c.tension_stiffening { 7 } else { 0 }
        ));
        // the designer's strain limit is this program's own attribute: written only when changed
        if (c.ecu - 3.5).abs() > 1e-9 {
            o.push_str(&format!("\t\t\t\tecu\t\t=\"{:.2}\"\r\n", c.ecu));
        }
        o.push_str(&format!("\t\t\t\tname\t\t=\"{}\"\t/>\r\n", escape(&c.name)));
    }
    for r in &sec.rebars {
        let elim = r.e_lim.map(|v| format!("\t\t\t\telim\t\t=\"{:.2}\"\r\n", v)).unwrap_or_default();
        o.push_str(&format!(
            "\t\t<rebar\t\tfy\t\t=\"{:.1}\"\r\n\t\t\t\tfu\t\t=\"{:.1}\"\r\n\t\t\t\tE\t\t=\"{:.0}\"\r\n\t\t\t\tesh\t\t=\"{:.1}\"\r\n\t\t\t\teu\t\t=\"{:.1}\"\r\n{}\t\t\t\tname\t\t=\"{}\"\t/>\r\n",
            r.fy, r.fu, r.es, r.esh, r.eu, elim, escape(&r.name)
        ));
    }
    for p in &sec.prestress {
        o.push_str(&format!(
            "\t\t<prestress\tfu\t\t=\"{:.1}\"\r\n\t\t\t\tE\t\t=\"{:.0}\"\r\n\t\t\t\tA\t\t=\"{:.3}\"\r\n\t\t\t\tB\t\t=\"{:.1}\"\r\n\t\t\t\tC\t\t=\"{:.1}\"\r\n\t\t\t\teu\t\t=\"{:.1}\"\r\n\t\t\t\tname\t\t=\"{}\"\t/>\r\n",
            p.fpu, p.ep, p.a, p.b, p.c, p.eu, escape(&p.name)
        ));
    }
    let cname = sec.concretes.first().map(|c| c.name.clone()).unwrap_or_default();
    o.push_str("\t\t<sectionSOLID\t\t>\r\n");
    for (i, (z, b)) in sec.outline.iter().rev().enumerate() {
        if i + 1 == sec.outline.len() {
            o.push_str(&format!("\t\t\t\t\t{:.1}\t{:.1}\t\"{}\"\r\n", z, b, escape(&cname)));
        } else {
            o.push_str(&format!("\t\t\t\t\t{:.1}\t{:.1}\r\n", z, b));
        }
    }
    o.push_str("\t\t\t</sectionSOLID>\r\n");
    let shape_pars: Option<(&str, Vec<f64>)> = match sec.shape {
        Shape::Rect { b, h } => Some(("RECT", vec![b, h])),
        Shape::Circle { d } => Some(("CIRCLE", vec![d, 600.0])),
        Shape::Tee { bf, hf, bw, h } => Some(("TBEAM", vec![bf, h, bw, hf])),
        Shape::IBeam { bt, ht, bw, bb, hb, h } => Some(("IBEAM", vec![bt, h, bw, ht, hb, bb])),
        Shape::Custom => None,
    };
    if let Some((name, pars)) = shape_pars {
        o.push_str(&format!("\t\t<shapesection\tname\t\t=\"{}\"", name));
        for (i, v) in pars.iter().enumerate() {
            o.push_str(&format!("\r\n\t\t\t\tpar{}\t\t=\"{:.1}\"", i + 1, v));
        }
        o.push_str("\t/>\r\n");
    }
    for l in &sec.long {
        let ring = if l.ring > 0 {
            format!(
                "\t\t\t\tpattern\t=\"1\"\r\n\t\t\t\tnround\t=\"{}\"\r\n\t\t\t\taligned\t=\"1\"\r\n\t\t\t\tindex\t\t=\"{}\"\r\n",
                l.ring, l.ring_id
            )
        } else {
            String::new()
        };
        let ring = if l.from_top { format!("{}\t\t\t\tfromtop\t=\"1\"\r\n", ring) } else { ring };
        o.push_str(&format!(
            "\t\t<longreinfx\tz\t\t=\"{:.2}\"\r\n\t\t\t\ttype\t\t=\"{}\"\r\n\t\t\t\tspace\t\t=\"0.0\"\r\n\t\t\t\tnum\t\t=\"{}\"\r\n\t\t\t\tA\t\t=\"{:.1}\"\r\n\t\t\t\tbartitle\t=\"{}\"\r\n\t\t\t\tdep\t\t=\"{:.2}\"\r\n{}\t\t\t\tname\t\t=\"{}\"\t/>\r\n",
            l.z, escape(&l.steel), l.num, l.area, escape(&l.bartitle), l.dep, ring, escape(&l.name)
        ));
    }
    if let Some(t) = &sec.stirrups {
        o.push_str(&format!(
            "\t\t<transreinfz\tA\t\t=\"{:.1}\"\r\n\t\t\t\ttype\t\t=\"{}\"\r\n\t\t\t\tpattern\t=\"{}\"\r\n\t\t\t\tspace\t\t=\"{:.1}\"\r\n\t\t\t\tdisttop\t=\"{:.1}\"\r\n\t\t\t\tdistbot\t=\"{:.1}\"\r\n\t\t\t\tbartitle\t=\"{}\"\r\n\t\t\t\tdb\t\t=\"{:.1}\"\r\n\t\t\t\tdep\t\t=\"0.00\"\r\n\t\t\t\tname\t\t=\"{}\"\t/>\r\n",
            t.area, escape(&t.steel), t.pattern, t.spacing, t.z_top, t.z_bot, escape(&t.bartitle), t.db, escape(&t.name)
        ));
    }
    let l = &sec.loads;
    o.push_str(&format!(
        "\t\t<sectloading\tBN\t\t=\"{:.1}\"\r\n\t\t\t\tBM\t\t=\"{:.1}\"\r\n\t\t\t\tBV\t\t=\"{:.1}\"\r\n\t\t\t\tMN\t\t=\"{:.3}\"\r\n\t\t\t\tMM\t\t=\"{:.3}\"\r\n\t\t\t\tMV\t\t=\"{:.3}\"\t/>\r\n",
        l.axial, l.m_const, l.v_const, l.n_inc, l.m_inc, l.v_inc
    ));
    // the original writes the material factors only for a factored run (0x514a20, "%3.2f")
    if let Some(a) = &sec.aci {
        // ACI 318 design: materials unfactored (phic/phis/phip 1.00 for the original), the
        // strength reduction factors in this program's own attributes
        o.push_str(&format!(
            "\t\t<analysisoptions\tphic\t=\"1.00\"\r\n\t\t\t\tphis\t=\"1.00\"\r\n\t\t\t\tphip\t=\"1.00\"\r\n\t\t\t\tdesign\t=\"{}\"\r\n\t\t\t\tphit\t=\"{:.2}\"\r\n\t\t\t\tphitps\t=\"{:.2}\"\r\n\t\t\t\tphitor\t=\"{:.2}\"\r\n\t\t\t\tphitied\t=\"{:.2}\"\r\n\t\t\t\tphispiral\t=\"{:.2}\"\r\n\t\t\t\tspiral\t=\"{}\"\r\n\t\t\t\tphiv\t=\"{:.2}\"\r\n\t\t\t\tecu\t=\"{:.2}\"\r\n\t\t\t\tpmaxtied\t=\"{:.2}\"\r\n\t\t\t\tpmaxspiral\t=\"{:.2}\"\t/>\r\n",
            if a.code == 1 { "AASHTO" } else { "ACI318" }, a.phi_tension, a.phi_tension_ps, a.phi_torsion, a.phi_tied, a.phi_spiral, a.spiral as u8, a.phi_shear, a.ecu * 1000.0, a.pmax_tied, a.pmax_spiral
        ));
    } else if sec.phi.iter().sum::<f64>() < 3.0 - 1e-9 {
        o.push_str(&format!(
            "\t\t<analysisoptions\tphic\t=\"{:3.2}\"\r\n\t\t\t\tphis\t=\"{:3.2}\"\r\n\t\t\t\tphip\t=\"{:3.2}\"\t/>\r\n",
            sec.phi[0], sec.phi[1], sec.phi[2]
        ));
    }
    o.push_str("\r\n\t</r3g_beam>\r\n</document>\r\n");
    o
}
