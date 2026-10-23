//! A small, dependency-free PDF writer for the printable parts of exports.
//!
//! Pages are US Letter. Text uses the standard Helvetica faces every PDF
//! reader carries, so no font is embedded; it is encoded as WinAnsi, and a
//! character outside that set prints as "?". Coordinates here run top-down in
//! points from the page's top-left corner and are flipped only when written.

use std::fmt::Write as _;

pub const PAGE_W: f32 = 612.0;
pub const PAGE_H: f32 = 792.0;
pub const MARGIN: f32 = 54.0;
pub const CONTENT_W: f32 = PAGE_W - 2.0 * MARGIN;
/// Lowest point body content may reach; the footer sits below it.
const BOTTOM: f32 = PAGE_H - MARGIN - 6.0;
const ROW_H: f32 = 18.0;
const PAD: f32 = 6.0;
const HERO_H: f32 = 132.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    fn ops(self) -> String {
        format!("{:.3} {:.3} {:.3}", self.0 as f32 / 255.0, self.1 as f32 / 255.0, self.2 as f32 / 255.0)
    }
}

/// The app's light palette (app/src/styles/app.css).
pub mod color {
    use super::Rgb;
    pub const INK: Rgb = Rgb(0x0b, 0x1b, 0x33);
    pub const INK_2: Rgb = Rgb(0x4a, 0x5a, 0x75);
    pub const INK_3: Rgb = Rgb(0x85, 0x92, 0xa8);
    pub const LINE: Rgb = Rgb(0xe6, 0xea, 0xf1);
    pub const SURFACE_2: Rgb = Rgb(0xf7, 0xf9, 0xfc);
    pub const BRAND: Rgb = Rgb(0x0a, 0xa2, 0xc0);
    pub const BRAND_STRONG: Rgb = Rgb(0x08, 0x7f, 0x99);
    pub const BRAND_SOFT: Rgb = Rgb(0xe2, 0xf5, 0xf9);
    pub const BRAND_SOFT_INK: Rgb = Rgb(0x07, 0x6a, 0x80);
    pub const POS: Rgb = Rgb(0x0e, 0x9f, 0x6e);
    pub const NEG: Rgb = Rgb(0xe0, 0x47, 0x4c);
    pub const WARN: Rgb = Rgb(0xc7, 0x77, 0x00);
    pub const WARN_SOFT: Rgb = Rgb(0xff, 0xf4, 0xe0);
    pub const WHITE: Rgb = Rgb(0xff, 0xff, 0xff);
    /// The hero gradient's stops, and its softened text color.
    pub const HERO: [(f32, Rgb); 3] = [(0.0, INK), (0.58, Rgb(0x12, 0x35, 0x5e)), (1.0, Rgb(0x0b, 0x8a, 0xa8))];
    pub const HERO_INK_2: Rgb = Rgb(0xb6, 0xc2, 0xd2);
}
use color::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Font {
    Regular,
    Bold,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

pub struct Col {
    pub head: &'static str,
    pub width: f32,
    pub align: Align,
}

impl Col {
    pub const fn left(head: &'static str, width: f32) -> Col {
        Col { head, width, align: Align::Left }
    }
    pub const fn right(head: &'static str, width: f32) -> Col {
        Col { head, width, align: Align::Right }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Plain,
    /// Bold, for subtotals.
    Strong,
    /// Bold on a tinted band with a rule above, for the bottom line.
    Total,
    /// Bold on a light band, heading the rows that follow it.
    Group,
    /// Greyed out, for voided or explanatory rows.
    Muted,
}

pub struct Row {
    cells: Vec<String>,
    style: Style,
    accent: Option<(usize, Rgb)>,
    keep: usize,
}

impl Row {
    pub fn new(style: Style, cells: &[&str]) -> Row {
        Row { cells: cells.iter().map(|c| c.to_string()).collect(), style, accent: None, keep: 0 }
    }
    /// Colors one cell.
    pub fn accent(mut self, col: usize, c: Rgb) -> Row {
        self.accent = Some((col, c));
        self
    }
    /// Keeps this row on the same page as the next `n` (a few at most).
    pub fn keep(mut self, n: usize) -> Row {
        self.keep = n;
        self
    }
}

pub struct Doc {
    pages: Vec<String>,
    y: f32,
    title: String,
    footer: String,
}

impl Doc {
    pub fn new(title: &str, footer: &str) -> Doc {
        Doc { pages: vec![String::new()], y: MARGIN, title: title.into(), footer: footer.into() }
    }

    fn out(&mut self) -> &mut String {
        self.pages.last_mut().expect("a page")
    }

    pub fn new_page(&mut self) {
        self.pages.push(String::new());
        self.y = MARGIN;
    }

    /// Starts a new page unless `h` more points fit on this one; true if it did.
    pub fn room(&mut self, h: f32) -> bool {
        if self.y + h > BOTTOM && self.y > MARGIN {
            self.new_page();
            true
        } else {
            false
        }
    }

    /// The full-bleed band across the top of the current page.
    pub fn hero(&mut self, kicker: &str, title: &str, sub: &str) {
        let out = self.out();
        // The gradient is the page's /Sh1 shading, clipped to the band.
        let _ = writeln!(out, "q 0 {:.2} {PAGE_W:.2} {HERO_H:.2} re W n /Sh1 sh Q", PAGE_H - HERO_H);
        fill(out, 0.0, HERO_H, PAGE_W, 4.0, BRAND);
        put_text(out, MARGIN, 50.0, 8.0, Font::Bold, BRAND_SOFT, 1.6, kicker);
        put_text(out, MARGIN, 82.0, 24.0, Font::Bold, WHITE, 0.0, &fit(title, CONTENT_W, 24.0, Font::Bold));
        put_text(out, MARGIN, 106.0, 10.5, Font::Regular, HERO_INK_2, 0.0, sub);
        self.y = HERO_H + 4.0 + 28.0;
    }

    /// Big-number tiles, three to a row.
    pub fn tiles(&mut self, tiles: &[(&str, String, Rgb)]) {
        let (gap, h) = (12.0, 58.0);
        let w = (CONTENT_W - 2.0 * gap) / 3.0;
        for chunk in tiles.chunks(3) {
            self.room(h);
            let y = self.y;
            let out = self.out();
            for (i, (label, value, c)) in chunk.iter().enumerate() {
                let x = MARGIN + i as f32 * (w + gap);
                fill(out, x, y, w, h, SURFACE_2);
                stroke_rect(out, x, y, w, h, LINE);
                put_text(out, x + 12.0, y + 19.0, 7.0, Font::Bold, INK_2, 0.6, &fit(&label.to_uppercase(), w - 24.0, 7.0, Font::Bold));
                put_text(out, x + 12.0, y + 44.0, 17.0, Font::Bold, *c, 0.0, &fit(value, w - 24.0, 17.0, Font::Bold));
            }
            self.y += h + gap;
        }
        self.y += 6.0;
    }

    /// A tinted callout with a colored edge.
    pub fn note(&mut self, title: &str, body: &str, bg: Rgb, fg: Rgb) {
        let lines = wrap(body, CONTENT_W - 32.0, 9.0, Font::Regular);
        let h = 34.0 + lines.len() as f32 * 12.5;
        self.room(h);
        let y = self.y;
        let out = self.out();
        fill(out, MARGIN, y, CONTENT_W, h, bg);
        fill(out, MARGIN, y, 3.0, h, fg);
        put_text(out, MARGIN + 16.0, y + 20.0, 10.0, Font::Bold, fg, 0.0, title);
        for (i, l) in lines.iter().enumerate() {
            put_text(out, MARGIN + 16.0, y + 35.0 + i as f32 * 12.5, 9.0, Font::Regular, INK_2, 0.0, l);
        }
        self.y += h + 18.0;
    }

    /// A section title with an accent bar, and an optional line of explanation.
    pub fn heading(&mut self, title: &str, sub: Option<&str>) {
        if self.y > MARGIN {
            self.y += 12.0;
        }
        self.room(ROW_H * 4.0 + 44.0);
        let y = self.y;
        let out = self.out();
        fill(out, MARGIN, y, 3.0, 15.0, BRAND);
        put_text(out, MARGIN + 11.0, y + 12.5, 14.0, Font::Bold, INK, 0.0, title);
        self.y += 24.0;
        if let Some(sub) = sub {
            self.para(sub, 9.0, INK_2);
        }
        self.y += 6.0;
    }

    pub fn gap(&mut self, h: f32) {
        self.y += h;
    }

    /// Wrapped body text across the content width.
    pub fn para(&mut self, s: &str, size: f32, c: Rgb) {
        let leading = size * 1.4;
        for l in wrap(s, CONTENT_W, size, Font::Regular) {
            self.room(leading);
            let y = self.y + size;
            put_text(self.out(), MARGIN, y, size, Font::Regular, c, 0.0, &l);
            self.y += leading;
        }
    }

    /// A table whose header repeats on every page it runs onto. A left-aligned
    /// cell may run into the empty cells to its right.
    pub fn table(&mut self, cols: &[Col], rows: &[Row], zebra: bool) {
        self.room(ROW_H * 2.0);
        self.table_head(cols);
        for (i, row) in rows.iter().enumerate() {
            if self.room(ROW_H * (1 + row.keep.min(6)) as f32) {
                self.table_head(cols);
            }
            let y = self.y;
            let bold = matches!(row.style, Style::Strong | Style::Total | Style::Group);
            let ink = if row.style == Style::Muted { INK_3 } else { INK };
            let out = self.out();
            match row.style {
                Style::Total => {
                    fill(out, MARGIN, y, CONTENT_W, ROW_H, BRAND_SOFT);
                    rule(out, MARGIN, MARGIN + CONTENT_W, y, INK, 0.8);
                }
                Style::Group => fill(out, MARGIN, y, CONTENT_W, ROW_H, SURFACE_2),
                _ if zebra && i % 2 == 1 => fill(out, MARGIN, y, CONTENT_W, ROW_H, SURFACE_2),
                _ => {}
            }
            let font = if bold { Font::Bold } else { Font::Regular };
            let mut x = MARGIN;
            for (j, col) in cols.iter().enumerate() {
                let cell = row.cells.get(j).map_or("", |c| c.as_str());
                if !cell.is_empty() {
                    let mut w = col.width;
                    if col.align == Align::Left {
                        w += cols.iter().enumerate().skip(j + 1).take_while(|(k, _)| row.cells.get(*k).is_none_or(|c| c.is_empty())).map(|(_, c)| c.width).sum::<f32>();
                    }
                    let c = row.accent.filter(|a| a.0 == j).map_or(ink, |a| a.1);
                    let s = fit(cell, w - 2.0 * PAD, 8.5, font);
                    let tx = match col.align {
                        Align::Left => x + PAD,
                        Align::Right => x + col.width - PAD - width(&s, 8.5, font),
                    };
                    put_text(out, tx, y + 12.2, 8.5, font, c, 0.0, &s);
                }
                x += col.width;
            }
            if row.style != Style::Total {
                rule(out, MARGIN, MARGIN + CONTENT_W, y + ROW_H, LINE, 0.5);
            }
            self.y += ROW_H;
        }
        self.y += 10.0;
    }

    fn table_head(&mut self, cols: &[Col]) {
        let y = self.y;
        let out = self.out();
        fill(out, MARGIN, y, CONTENT_W, ROW_H, BRAND_SOFT);
        let mut x = MARGIN;
        for col in cols {
            let s = fit(&col.head.to_uppercase(), col.width - 2.0 * PAD, 7.0, Font::Bold);
            let tx = match col.align {
                Align::Left => x + PAD,
                Align::Right => x + col.width - PAD - width(&s, 7.0, Font::Bold) - 0.5 * s.chars().count() as f32,
            };
            put_text(out, tx, y + 11.8, 7.0, Font::Bold, BRAND_SOFT_INK, 0.5, &s);
            x += col.width;
        }
        self.y += ROW_H;
    }

    /// Adds the running footer to every page and serializes the document.
    pub fn finish(mut self) -> Vec<u8> {
        let n = self.pages.len();
        for (i, page) in self.pages.iter_mut().enumerate() {
            let y = PAGE_H - 34.0;
            rule(page, MARGIN, PAGE_W - MARGIN, y - 12.0, LINE, 0.5);
            let num = format!("Page {} of {n}", i + 1);
            let nw = width(&num, 7.5, Font::Regular);
            put_text(page, MARGIN, y, 7.5, Font::Regular, INK_3, 0.0, &fit(&self.footer, CONTENT_W - nw - 24.0, 7.5, Font::Regular));
            put_text(page, PAGE_W - MARGIN - nw, y, 7.5, Font::Regular, INK_3, 0.0, &num);
        }
        serialize(&self.title, &self.pages)
    }
}

fn fill(out: &mut String, x: f32, y: f32, w: f32, h: f32, c: Rgb) {
    let _ = writeln!(out, "{} rg {x:.2} {:.2} {w:.2} {h:.2} re f", c.ops(), PAGE_H - y - h);
}

fn stroke_rect(out: &mut String, x: f32, y: f32, w: f32, h: f32, c: Rgb) {
    let _ = writeln!(out, "{} RG 0.75 w {x:.2} {:.2} {w:.2} {h:.2} re S", c.ops(), PAGE_H - y - h);
}

fn rule(out: &mut String, x1: f32, x2: f32, y: f32, c: Rgb, w: f32) {
    let y = PAGE_H - y;
    let _ = writeln!(out, "{} RG {w:.2} w {x1:.2} {y:.2} m {x2:.2} {y:.2} l S", c.ops());
}

/// `y` is the baseline; `tracking` is extra space between letters.
#[allow(clippy::too_many_arguments)]
fn put_text(out: &mut String, x: f32, y: f32, size: f32, font: Font, c: Rgb, tracking: f32, s: &str) {
    let f = if font == Font::Bold { "F2" } else { "F1" };
    let _ = write!(out, "BT /{f} {size:.2} Tf {tracking:.2} Tc {} rg {x:.2} {:.2} Td (", c.ops(), PAGE_H - y);
    for ch in s.chars() {
        match win_ansi(ch) {
            b @ (b'(' | b')' | b'\\') => {
                out.push('\\');
                out.push(b as char);
            }
            b @ 0x20..=0x7e => out.push(b as char),
            b => {
                let _ = write!(out, "\\{b:03o}");
            }
        }
    }
    out.push_str(") Tj ET\n");
}

fn serialize(title: &str, pages: &[String]) -> Vec<u8> {
    // The hero gradient, left to right across the page, as an axial shading.
    let stop = |a: Rgb, b: Rgb| format!("<< /FunctionType 2 /Domain [0 1] /C0 [{}] /C1 [{}] /N 1 >>", a.ops(), b.ops());
    let hero = format!(
        "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 {PAGE_W} 0] /Extend [true true] /Function << /FunctionType 3 /Domain [0 1] /Functions [{} {}] /Bounds [{}] /Encode [0 1 0 1] >> >>",
        stop(HERO[0].1, HERO[1].1),
        stop(HERO[1].1, HERO[2].1),
        HERO[1].0
    );
    let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    let mut obj = |pdf: &mut Vec<u8>, body: &str| {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", offsets.len()).as_bytes());
    };
    // 1 catalog, 2 page tree, 3–4 fonts, 5 info, then a page and its contents per page.
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", 6 + 2 * i)).collect();
    obj(&mut pdf, "<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut pdf, &format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()));
    for face in ["Helvetica", "Helvetica-Bold"] {
        obj(&mut pdf, &format!("<< /Type /Font /Subtype /Type1 /BaseFont /{face} /Encoding /WinAnsiEncoding >>"));
    }
    let mut t = String::new();
    put_text(&mut t, 0.0, 0.0, 0.0, Font::Regular, INK, 0.0, title);
    let title = &t[t.find('(').unwrap_or(0)..t.rfind(')').map_or(t.len(), |i| i + 1)];
    obj(&mut pdf, &format!("<< /Title {title} /Producer (Tallybook) >>"));
    for (i, content) in pages.iter().enumerate() {
        obj(
            &mut pdf,
            &format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> /Shading << /Sh1 {hero} >> >> /Contents {} 0 R >>",
                7 + 2 * i
            ),
        );
        obj(&mut pdf, &format!("<< /Length {} >>\nstream\n{content}endstream", content.len()));
    }
    let xref = pdf.len();
    let mut tail = format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
    for o in &offsets {
        let _ = writeln!(tail, "{o:010} 00000 n ");
    }
    let _ = write!(tail, "trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1);
    pdf.extend_from_slice(tail.as_bytes());
    pdf
}

fn win_ansi(c: char) -> u8 {
    match c {
        ' '..='~' => c as u8,
        '\u{a0}'..='\u{ff}' => c as u32 as u8,
        '€' => 0x80,
        '…' => 0x85,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '™' => 0x99,
        '−' => b'-',
        '\t' | '\n' | '\r' => b' ',
        _ => b'?',
    }
}

/// Advance widths in thousandths of an em, from the Adobe font metrics, for
/// the printable ASCII range.
#[rustfmt::skip]
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015,
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611,
    278, 278, 278, 469, 556, 333,
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500,
    334, 260, 334, 584,
];
#[rustfmt::skip]
const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975,
    722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611,
    333, 278, 333, 584, 556, 333,
    556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500,
    389, 280, 389, 584,
];

fn advance(b: u8, font: Font) -> u16 {
    let bold = font == Font::Bold;
    match b {
        0x20..=0x7e => (if bold { HELVETICA_BOLD } else { HELVETICA })[(b - 0x20) as usize],
        0x85 | 0x97 | 0x99 => 1000,
        0x91 | 0x92 => if bold { 278 } else { 222 },
        0x93 | 0x94 => if bold { 500 } else { 333 },
        0x95 => 350,
        0xa0 | 0xb7 => 278,
        _ => 556,
    }
}

pub fn width(s: &str, size: f32, font: Font) -> f32 {
    s.chars().map(|c| advance(win_ansi(c), font) as f32).sum::<f32>() * size / 1000.0
}

/// `s` cut down with an ellipsis until it fits in `w`.
pub fn fit(s: &str, w: f32, size: f32, font: Font) -> String {
    if width(s, size, font) <= w {
        return s.to_string();
    }
    let mut out: String = s.to_string();
    while !out.is_empty() && width(&out, size, font) + width("…", size, font) > w {
        out.pop();
    }
    format!("{}…", out.trim_end())
}

/// `s` broken into lines no wider than `w`, at spaces where it can be.
pub fn wrap(s: &str, w: f32, size: f32, font: Font) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        let next = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if width(&next, size, font) <= w || line.is_empty() {
            line = next;
        } else {
            lines.push(std::mem::replace(&mut line, word.to_string()));
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.into_iter().map(|l| fit(&l, w, size, font)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_cover_ascii_and_fit_trims() {
        assert_eq!(width("0", 1000.0, Font::Regular), 556.0);
        assert_eq!(width("W", 1000.0, Font::Bold), 944.0);
        let s = fit("A memo far too long to fit in the column", 60.0, 8.5, Font::Regular);
        assert!(s.ends_with('…') && width(&s, 8.5, Font::Regular) <= 60.0, "{s}");
        assert_eq!(wrap("one two three", 30.0, 10.0, Font::Regular), ["one", "two", "three"]);
    }

    #[test]
    fn xref_points_at_every_object() {
        let mut d = Doc::new("Test (with parens) · ok", "footer");
        d.hero("KICKER", "Title", "Sub");
        let rows: Vec<Row> = (0..80).map(|i| Row::new(Style::Plain, &[&i.to_string(), "café", "$1.00"])).collect();
        d.table(&[Col::left("A", 100.0), Col::left("B", 300.0), Col::right("C", 104.0)], &rows, true);
        let pdf = d.finish();
        // Offsets are bytes, and the header's marker bytes aren't UTF-8, so search bytes.
        let find = |needle: &[u8]| pdf.windows(needle.len()).rposition(|w| w == needle).unwrap();
        assert!(pdf.starts_with(b"%PDF-1.4") && pdf.ends_with(b"%%EOF\n"));
        assert!(find(b"/Count 3") > 0, "80 rows run over three pages");
        let tail = std::str::from_utf8(&pdf[find(b"startxref")..]).unwrap();
        let start: usize = tail.lines().nth(1).unwrap().parse().unwrap();
        let table = std::str::from_utf8(&pdf[start..]).unwrap();
        assert!(table.starts_with("xref"));
        for (n, line) in table.lines().skip(3).take_while(|l| l.ends_with(" n ")).enumerate() {
            let off: usize = line[..10].parse().unwrap();
            assert!(pdf[off..].starts_with(format!("{} 0 obj", n + 1).as_bytes()), "object {}", n + 1);
        }
    }
}
