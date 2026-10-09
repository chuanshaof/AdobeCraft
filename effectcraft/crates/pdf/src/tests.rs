//! Test files are generated here (no third-party samples).

use super::*;

use crate::write::pdf;

const CONTENT: &str = "/OC /MC0 BDC\n0 1 0 rg 0 0 200 100 re f\nEMC\n\
/OC /MC1 BDC\nq 10 10 80 80 re W n\n1 0 0 rg 0 0 50 100 re f\nQ\n\
/Pattern cs /P0 scn 100 0 100 50 re f\n\
0 0 1 RG 4 w 120 80 m 180 80 l S\n\
BT /F1 12 Tf (ignored) Tj ET\nEMC\n";

pub(crate) fn sample_pdf(object_stream: bool) -> Vec<u8> {
    let page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Properties << /MC0 5 0 R /MC1 6 0 R >> /Pattern << /P0 7 0 R >> >> /Contents 4 0 R >>";
    let pattern = "<< /PatternType 2 /Shading << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [100 0 200 0] /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> /Extend [true true] >> >>";
    let mut objs = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_string(), None),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(), None),
        (3, page.to_string(), None),
        (4, "<< >>".to_string(), Some(CONTENT.as_bytes().to_vec())),
        (7, pattern.to_string(), None),
    ];
    let ocg5 = "<< /Type /OCG /Name (Background) >>";
    let ocg6 = "<< /Type /OCG /Name <FEFF00410072007400> >>"; // "Art" in UTF-16BE
    if object_stream {
        let body = format!("{ocg5} {ocg6}");
        let head = format!("5 0 6 {} ", ocg5.len() + 1);
        let data = format!("{head}{body}");
        objs.push((8, format!("<< /Type /ObjStm /N 2 /First {} >>", head.len()), Some(data.into_bytes())));
    } else {
        objs.push((5, ocg5.to_string(), None));
        objs.push((6, ocg6.to_string(), None));
    }
    pdf(&objs, 1)
}

fn px(doc: &Doc, x: i64, y: i64) -> [f32; 4] {
    let (w, h) = doc.pixel_size();
    let img = effectcraft_svg::rasterize(doc, w, h, 1.0);
    img.get(x, y)
}

#[test]
fn pdf_paths_clip_gradient_and_layers() {
    for objstm in [false, true] {
        let bytes = sample_pdf(objstm);
        assert_eq!(sniff(&bytes), Some(Format::Pdf));
        assert_eq!(page_count(&bytes), 1);
        let doc = parse(&bytes).unwrap();
        assert_eq!((doc.width, doc.height), (200.0, 100.0));
        assert_eq!(layer_names(&doc), vec!["Background", "Art"], "object stream {objstm}");
        assert!(doc.skipped.contains(&"text (no font)".to_string()), "{:?}", doc.skipped);
        let (w, h) = doc.pixel_size();
        let img = effectcraft_svg::rasterize(&doc, w, h, 1.0);
        let at = |x: i64, y: i64| img.get(x, y);
        // Red rectangle clipped to x ≥ 10, over the green background.
        assert!(at(30, 50)[0] > 0.99 && at(30, 50)[1] < 0.01, "{:?}", at(30, 50));
        assert!(at(5, 50)[1] > 0.99 && at(5, 50)[0] < 0.01, "clipped out: {:?}", at(5, 50));
        assert!(at(70, 50)[1] > 0.99, "{:?}", at(70, 50));
        // Axial shading, red → blue left to right (in the lower half: PDF y is up).
        let (l, r) = (at(110, 75), at(190, 75));
        assert!(l[0] > 0.8 && l[2] < 0.2 && r[2] > 0.8 && r[0] < 0.2, "{l:?} {r:?}");
        assert!(at(150, 25)[1] > 0.99, "the gradient stays in its rectangle: {:?}", at(150, 25));
        // Stroked line (PDF y = 80 → 20 px from the top).
        assert!(at(150, 20)[2] > 0.99 && at(150, 20)[1] < 0.01, "{:?}", at(150, 20));
        // One layer at a time.
        let bg = layer_doc(&doc, 0);
        assert!(px(&bg, 30, 50)[1] > 0.99);
        let art = layer_doc(&doc, 1);
        assert_eq!(px(&art, 5, 50)[3], 0.0);
        // Continuous rasterisation: twice the size stays sharp at the clip edge.
        let big = effectcraft_svg::rasterize(&doc, 400, 200, 2.0);
        assert!(big.get(21, 100)[0] > 0.99 && big.get(18, 100)[1] > 0.99);
    }
}

#[test]
fn pdf_forms_rotation_cmyk_and_alpha() {
    let content = "q 2 0 0 2 0 0 cm /Fm0 Do Q\n/GS0 gs 0 0 0 1 k 0 0 10 10 re f\n";
    let form = "0 1 1 0 k 0 0 20 20 re f";
    let objs = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_string(), None),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 /Rotate 90 >>".to_string(), None),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 50] /Resources << /XObject << /Fm0 5 0 R >> /ExtGState << /GS0 << /ca 0.5 >> >> >> /Contents 4 0 R >>"
                .to_string(),
            None,
        ),
        (4, "<< >>".to_string(), Some(content.as_bytes().to_vec())),
        (5, "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] >>".to_string(), Some(form.as_bytes().to_vec())),
    ];
    let doc = parse(&pdf(&objs, 1)).unwrap();
    // Rotated a quarter turn: 50 × 100.
    assert_eq!((doc.width, doc.height), (50.0, 100.0));
    let (w, h) = doc.pixel_size();
    let img = effectcraft_svg::rasterize(&doc, w, h, 1.0);
    // The form (CMYK red, clipped to its 10×10 box, scaled ×2) covers PDF (0..20, 0..20); the
    // half-transparent black square covers PDF (0..10, 0..10). /Rotate 90: PDF (x, y) → (y, x).
    let red = img.get(15, 15);
    assert!(red[0] > 0.99 && red[1] < 0.01 && red[3] > 0.99, "{red:?}");
    let dark = img.get(5, 5);
    assert!((dark[0] - 0.5).abs() < 0.02 && dark[3] > 0.99, "{dark:?}");
    assert_eq!(img.get(30, 30)[3], 0.0, "form clipped to its bounding box");
}

const EPS: &str = "%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 100 100\n%%EndComments\n\
/m {moveto} bind def /l {lineto} bind def\n\
/cm { 6 array astore concat } bind def\n\
/box { 4 dict begin /h exch def /w exch def /y exch def /x exch def x y m w 0 rlineto 0 h rlineto w neg 0 rlineto closepath end } bind def\n\
1 0 0 setrgbcolor\n10 10 30 30 box fill\n\
gsave 0 0 1 setrgbcolor 70 50 20 0 360 arc fill grestore\n\
gsave 1 0 0 1 50 0 cm 0 1 0 setrgbcolor 0 0 10 10 box fill grestore\n\
0 setgray 2 setlinewidth 0 90 m 100 90 l stroke\n\
/Helvetica findfont 12 scalefont setfont 5 5 moveto (text) show\n\
showpage\n%%EOF\n";

#[test]
fn eps_postscript_subset() {
    let doc = parse(EPS.as_bytes()).unwrap();
    assert_eq!((doc.width, doc.height), (100.0, 100.0));
    assert!(doc.skipped.is_empty(), "{:?}", doc.skipped);
    assert!(shape_names(&doc).contains(&"Text: text".to_string()), "EPS text is drawn: {:?}", shape_names(&doc));
    let at = |x, y| px(&doc, x, y);
    assert!(at(25, 75)[0] > 0.99 && at(25, 75)[3] > 0.99, "red box {:?}", at(25, 75));
    assert!(at(70, 50)[2] > 0.99, "blue disc {:?}", at(70, 50));
    assert!(at(55, 95)[1] > 0.99, "translated green box {:?}", at(55, 95));
    let line = at(50, 10);
    assert!(line[3] > 0.99 && line[0] < 0.01, "black line {line:?}");
    assert_eq!(at(95, 70)[3], 0.0);
    // DOS EPS binary header (with a fake TIFF preview after the PostScript).
    let ps = EPS.as_bytes();
    let mut dos = vec![0xC5, 0xD0, 0xD3, 0xC6];
    dos.extend_from_slice(&30u32.to_le_bytes());
    dos.extend_from_slice(&(ps.len() as u32).to_le_bytes());
    dos.extend_from_slice(&[0; 18]);
    dos.extend_from_slice(ps);
    dos.extend_from_slice(b"II*\0 not a real preview");
    let d2 = parse(&dos).unwrap();
    assert_eq!(d2.root.children.len(), doc.root.children.len());
    assert_eq!(codec("art.eps", &dos), Some("EPS"));
    assert_eq!(codec("art.ai", &sample_pdf(false)), Some("AI"));
    assert_eq!(parse(b"hello"), Err(Error::NotVector));
}

#[test]
fn filters_decode() {
    assert_eq!(object::ascii85(b"<~87cURD]i,\"Ebo80~>"), b"Hello World!".to_vec());
    let z = miniz_oxide::deflate::compress_to_vec_zlib(b"abc", 6);
    assert_eq!(object::inflate(&z).unwrap(), b"abc");
}

// ------------------------------------------------------------------ M13.6: text, images,
// patterns, soft masks, blend modes, pages

pub(crate) type Objs = Vec<(u32, String, Option<Vec<u8>>)>;

/// A one-page 200×100 PDF: `resources` is the page's resource dictionary body, `extra` more
/// objects (numbered from 10).
pub(crate) fn page_pdf(content: &str, resources: &str, extra: Objs) -> Vec<u8> {
    let mut objs: Objs = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into(), None),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(), None),
        (3, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << {resources} >> /Contents 4 0 R >>"), None),
        (4, "<< >>".into(), Some(content.as_bytes().to_vec())),
    ];
    objs.extend(extra);
    pdf(&objs, 1)
}

pub(crate) fn render(doc: &Doc) -> effectcraft_raster::Image {
    let (w, h) = doc.pixel_size();
    effectcraft_svg::rasterize(doc, w, h, 1.0)
}

pub(crate) fn coverage(img: &effectcraft_raster::Image, x0: i64, y0: i64, x1: i64, y1: i64) -> f32 {
    let mut s = 0.0;
    for y in y0..y1 {
        for x in x0..x1 {
            s += img.get(x, y)[3];
        }
    }
    s
}

pub(crate) fn shape_names(doc: &Doc) -> Vec<String> {
    fn walk(g: &effectcraft_svg::Group, out: &mut Vec<String>) {
        for c in &g.children {
            match c {
                Node::Group(s) => walk(s, out),
                Node::Shape(s) => out.push(s.name.clone()),
                Node::Image(i) => out.push(i.name.clone()),
            }
        }
    }
    let mut out = vec![];
    walk(&doc.root, &mut out);
    out
}

/// Type 2 charstring operands.
fn t2(v: &[i32]) -> Vec<u8> {
    let mut o = vec![];
    for &x in v {
        if (-107..=107).contains(&x) {
            o.push((x + 139) as u8);
        } else {
            o.push(28);
            o.extend_from_slice(&(x as i16).to_be_bytes());
        }
    }
    o
}

/// A CFF font with a 500-unit square glyph `square` (x 50..550, y 0..500, width 600).
fn square_cff() -> Vec<u8> {
    let mut sq = t2(&[600, 50, 0]);
    sq.push(21);
    sq.extend(t2(&[500, 500, -500]));
    sq.push(6);
    sq.push(14);
    crate::cff::write_test_cff(&[(".notdef", vec![14]), ("square", sq)])
}

const SQUARE_FONT: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Square /FirstChar 65 /LastChar 65 /Widths [600] /Encoding << /Differences [65 /square] >> /FontDescriptor 11 0 R >>";

pub(crate) fn square_font_objs() -> Objs {
    vec![
        (10, SQUARE_FONT.into(), None),
        (11, "<< /Type /FontDescriptor /FontName /Square /Flags 32 /FontFile3 12 0 R >>".into(), None),
        (12, "<< /Subtype /Type1C >>".into(), Some(square_cff())),
    ]
}

#[test]
fn text_with_an_embedded_cff_font_spacing_and_tj() {
    // Squares 10 pt per 100 units: A at 0..60 (square 5..55), Tc 10 and a TJ kern of −400
    // (+40) put the second at 110 (square 115..165).
    let bytes = page_pdf("BT /F1 100 Tf 0 0 Td 10 Tc [(A) -400 (A)] TJ ET", "/Font << /F1 10 0 R >>", square_font_objs());
    let doc = parse(&bytes).unwrap();
    assert!(!doc.skipped.iter().any(|s| s.contains("text")), "{:?}", doc.skipped);
    assert!(shape_names(&doc).iter().any(|n| n.starts_with("Text")), "{:?}", shape_names(&doc));
    let img = render(&doc);
    for (x, on) in [(30, true), (60, false), (105, false), (140, true), (170, false)] {
        assert_eq!(img.get(x, 75)[3] > 0.99, on, "x = {x}: {:?}", img.get(x, 75));
    }
    // Squares are 50 pt tall: rows 50..100.
    assert_eq!(img.get(30, 45)[3], 0.0);
}

#[test]
fn text_render_mode_clip() {
    let bytes = page_pdf("BT /F1 100 Tf 7 Tr 0 0 Td (A) Tj ET 1 0 0 rg 0 0 200 100 re f", "/Font << /F1 10 0 R >>", square_font_objs());
    let img = render(&parse(&bytes).unwrap());
    assert!(img.get(30, 75)[0] > 0.99 && img.get(30, 75)[3] > 0.99);
    assert_eq!(img.get(100, 75)[3], 0.0, "clipped to the glyph");
    assert_eq!(img.get(30, 25)[3], 0.0);
}

#[test]
fn text_with_an_embedded_type1_font() {
    // hsbw 50 600; 0 0 rmoveto; 500 hlineto 500 vlineto -500 hlineto closepath endchar
    let cs = vec![50 + 139, 248, 236, 13, 139, 139, 21, 248, 136, 6, 248, 136, 7, 252, 136, 6, 9, 14];
    let font = crate::type1::write_test_type1(&[(".notdef", vec![139, 139, 13, 14]), ("box", cs)], &[(66, "box")]);
    let extra: Objs = vec![
        (10, "<< /Type /Font /Subtype /Type1 /BaseFont /Box /FirstChar 66 /LastChar 66 /Widths [600] /FontDescriptor 11 0 R >>".into(), None),
        (11, "<< /Type /FontDescriptor /FontName /Box /Flags 4 /FontFile 12 0 R >>".into(), None),
        (12, "<< /Length1 0 /Length2 0 /Length3 0 >>".into(), Some(font)),
    ];
    // The font's built-in encoding maps B (66) to `box`.
    let img = render(&parse(&page_pdf("BT /F1 100 Tf 0 0 Td (BB) Tj ET", "/Font << /F1 10 0 R >>", extra)).unwrap());
    for (x, on) in [(30, true), (60, false), (90, true), (120, false)] {
        assert_eq!(img.get(x, 75)[3] > 0.99, on, "x = {x}");
    }
}

#[test]
fn text_with_a_type3_font() {
    let extra: Objs = vec![
        (
            10,
            "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 100 100] /FontMatrix [0.01 0 0 0.01 0 0] /CharProcs << /sq 11 0 R >> /Encoding << /Type /Encoding /Differences [65 /sq] >> /FirstChar 65 /LastChar 65 /Widths [100] >>".into(),
            None,
        ),
        (11, "<< >>".into(), Some(b"100 0 d0 0 0 100 100 re f".to_vec())),
    ];
    let img = render(&parse(&page_pdf("BT 0 0 1 rg /F1 20 Tf 10 10 Td (AA) Tj ET", "/Font << /F1 10 0 R >>", extra)).unwrap());
    // 20 pt squares from x = 10, advancing 20: user 10..30 and 30..50, y 10..30 (rows 70..90).
    assert!(img.get(20, 80)[2] > 0.99 && img.get(20, 80)[3] > 0.99, "{:?}", img.get(20, 80));
    assert!(img.get(40, 80)[3] > 0.99);
    assert_eq!(img.get(55, 80)[3], 0.0);
    assert_eq!(img.get(20, 60)[3], 0.0);
}

#[test]
fn text_with_standard_fonts_embedded_truetype_and_cid_fonts() {
    use skrifa::MetadataProvider;
    let inter = effectcraft_text::fonts::INTER_REGULAR.to_vec();
    // Not embedded: Helvetica drawn with the bundled sans serif.
    let std14 = page_pdf("BT /F1 60 Tf 10 30 Td (Hi) Tj ET", "/Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >>", vec![]);
    let doc = parse(&std14).unwrap();
    assert!(shape_names(&doc).contains(&"Text: Hi".to_string()), "{:?}", shape_names(&doc));
    assert!(doc.skipped.is_empty(), "{:?}", doc.skipped);
    let a = render(&doc);
    let ink = coverage(&a, 0, 0, 200, 100);
    assert!(ink > 300.0, "{ink}");
    // Ink only on the text line: baseline at row 70, cap height ≈ 44 px above it.
    assert!(coverage(&a, 0, 72, 200, 100) < 1.0 && coverage(&a, 0, 0, 200, 20) < 1.0);
    // The same face embedded (a TrueType font program, WinAnsiEncoding): the same outlines.
    let tt = page_pdf(
        "BT /F1 60 Tf 10 30 Td (Hi) Tj ET",
        "/Font << /F1 10 0 R >>",
        vec![
            (10, "<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Inter /Encoding /WinAnsiEncoding /FontDescriptor 11 0 R >>".into(), None),
            (11, "<< /Type /FontDescriptor /Flags 32 /FontFile2 12 0 R >>".into(), None),
            (12, "<< >>".into(), Some(inter.clone())),
        ],
    );
    let b = render(&parse(&tt).unwrap());
    assert!((coverage(&b, 0, 0, 200, 100) - ink).abs() < 1.0, "{} vs {ink}", coverage(&b, 0, 0, 200, 100));
    // A composite font: Identity-H codes are glyph ids of a CIDFontType2.
    let font = skrifa::FontRef::new(&inter).unwrap();
    let gid = |c: char| font.charmap().map(c).unwrap().to_u32() as u16;
    let (h, i) = (gid('H'), gid('i'));
    let loc = skrifa::instance::LocationRef::default();
    let upem = font.metrics(skrifa::instance::Size::unscaled(), loc).units_per_em as f32;
    let w = |g: u16| font.glyph_metrics(skrifa::instance::Size::unscaled(), loc).advance_width(skrifa::GlyphId::new(g as u32)).unwrap() / upem * 1000.0;
    let cid = page_pdf(
        &format!("BT /F1 60 Tf 10 30 Td <{h:04X}{i:04X}> Tj ET"),
        "/Font << /F1 10 0 R >>",
        vec![
            (10, "<< /Type /Font /Subtype /Type0 /BaseFont /Inter /Encoding /Identity-H /DescendantFonts [13 0 R] >>".into(), None),
            (
                13,
                format!(
                    "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Inter /CIDToGIDMap /Identity /W [{h} [{}] {i} [{}]] /FontDescriptor 11 0 R >>",
                    w(h),
                    w(i)
                ),
                None,
            ),
            (11, "<< /Type /FontDescriptor /Flags 32 /FontFile2 12 0 R >>".into(), None),
            (12, "<< >>".into(), Some(inter)),
        ],
    );
    let c = render(&parse(&cid).unwrap());
    assert!((coverage(&c, 0, 0, 200, 100) - ink).abs() < 1.0, "{} vs {ink}", coverage(&c, 0, 0, 200, 100));
}

#[test]
fn images_flate_smask_indexed_stencil_inline_and_dct() {
    let objs = |im: &str, data: Vec<u8>, extra: Objs| -> Vec<u8> {
        let mut e: Objs = vec![(10, im.to_string(), Some(data))];
        e.extend(extra);
        page_pdf("q 100 0 0 50 0 0 cm /Im1 Do Q", "/XObject << /Im1 10 0 R >>", e)
    };
    // RGB with a soft mask: red opaque, green transparent.
    let doc = parse(&objs(
        "<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /SMask 11 0 R >>",
        vec![255, 0, 0, 0, 255, 0],
        vec![(11, "<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 >>".into(), Some(vec![255, 0]))],
    ))
    .unwrap();
    assert!(doc.skipped.is_empty(), "{:?}", doc.skipped);
    let img = render(&doc);
    assert!(img.get(25, 75)[0] > 0.95 && img.get(25, 75)[3] > 0.95, "{:?}", img.get(25, 75));
    assert!(img.get(75, 75)[3] < 0.05, "{:?}", img.get(75, 75));
    assert_eq!(img.get(25, 25)[3], 0.0, "the image is the unit square of user space");
    // Indexed (palette in a string), 8 bits.
    let img = render(
        &parse(&objs(
            "<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace [/Indexed /DeviceRGB 1 <0000FFFFFF00>] /BitsPerComponent 8 >>",
            vec![1, 0],
            vec![],
        ))
        .unwrap(),
    );
    let (l, r) = (img.get(25, 75), img.get(75, 75));
    assert!(l[0] > 0.95 && l[1] > 0.95 && l[2] < 0.05 && r[2] > 0.95 && r[0] < 0.05, "{l:?} {r:?}");
    // ICCBased with an /Alternate space, 1-bit samples.
    let img = render(
        &parse(&objs(
            "<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace [/ICCBased 11 0 R] /BitsPerComponent 1 >>",
            vec![0b0100_0000],
            vec![(11, "<< /N 1 /Alternate /DeviceGray >>".into(), Some(vec![0; 16]))],
        ))
        .unwrap(),
    );
    assert!(img.get(25, 75)[0] < 0.05 && img.get(75, 75)[0] > 0.95);
    // Stencil mask painted in the fill colour (sample 0 paints).
    let stencil = page_pdf(
        "0 1 0 rg q 100 0 0 50 0 0 cm /Im1 Do Q",
        "/XObject << /Im1 10 0 R >>",
        vec![(10, "<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ImageMask true >>".into(), Some(vec![0b0100_0000]))],
    );
    let img = render(&parse(&stencil).unwrap());
    assert!(img.get(25, 75)[1] > 0.95 && img.get(25, 75)[3] > 0.95);
    assert!(img.get(75, 75)[3] < 0.05);
    // Inline image (abbreviated keys, ASCIIHex) in the top half.
    let inline = page_pdf("q 100 0 0 50 0 50 cm BI /W 2 /H 1 /CS /RGB /BPC 8 /F /AHx ID 00FF00FF0000> EI Q 0 0 1 rg 150 0 50 50 re f", "", vec![]);
    let doc = parse(&inline).unwrap();
    let img = render(&doc);
    assert!(img.get(25, 25)[1] > 0.95 && img.get(75, 25)[0] > 0.95, "{:?} {:?}", img.get(25, 25), img.get(75, 25));
    assert!(img.get(175, 75)[2] > 0.95, "content after EI still runs");
    // DCT (JPEG): left half red, right half blue.
    let mut jpg = vec![];
    let mut rgb = ::image::RgbImage::new(16, 16);
    for (x, _, p) in rgb.enumerate_pixels_mut() {
        *p = if x < 8 { ::image::Rgb([255, 0, 0]) } else { ::image::Rgb([0, 0, 255]) };
    }
    ::image::DynamicImage::ImageRgb8(rgb).write_to(&mut std::io::Cursor::new(&mut jpg), ::image::ImageFormat::Jpeg).unwrap();
    let doc =
        parse(&objs("<< /Type /XObject /Subtype /Image /Width 16 /Height 16 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode >>", jpg, vec![]))
            .unwrap();
    assert!(doc.skipped.is_empty(), "{:?}", doc.skipped);
    let img = render(&doc);
    let (l, r) = (img.get(20, 75), img.get(80, 75));
    assert!(l[0] > 0.85 && l[2] < 0.15 && r[2] > 0.85 && r[0] < 0.15, "{l:?} {r:?}");
}

#[test]
fn soft_masks_and_blend_modes() {
    // Luminosity: a form that is white on the left half, transparent (→ black backdrop) elsewhere.
    let lum = page_pdf(
        "q /GS1 gs 1 0 0 rg 0 0 200 100 re f Q 0 0 1 rg 0 0 20 20 re f",
        "/ExtGState << /GS1 << /SMask << /S /Luminosity /G 10 0 R >> >> >>",
        vec![(
            10,
            "<< /Type /XObject /Subtype /Form /BBox [0 0 200 100] /Group << /S /Transparency /CS /DeviceGray >> >>".into(),
            Some(b"1 g 0 0 100 100 re f".to_vec()),
        )],
    );
    let doc = parse(&lum).unwrap();
    assert!(doc.skipped.is_empty(), "{:?}", doc.skipped);
    let img = render(&doc);
    assert!(img.get(50, 50)[0] > 0.99 && img.get(50, 50)[3] > 0.99, "{:?}", img.get(50, 50));
    assert!(img.get(150, 50)[3] < 0.01, "{:?}", img.get(150, 50));
    assert!(img.get(10, 90)[2] > 0.99, "the mask ends with Q");
    // Alpha: the mask form's coverage.
    let alpha = page_pdf(
        "/GS1 gs 1 0 0 rg 0 0 200 100 re f",
        "/ExtGState << /GS1 << /SMask << /S /Alpha /G 10 0 R >> >> >>",
        vec![(10, "<< /Type /XObject /Subtype /Form /BBox [0 0 200 100] >>".into(), Some(b"0 0 0 rg 100 0 100 100 re f".to_vec()))],
    );
    let img = render(&parse(&alpha).unwrap());
    assert!(img.get(50, 50)[3] < 0.01 && img.get(150, 50)[0] > 0.99);
    // Multiply over a light blue backdrop; Screen in the right half.
    let blend = page_pdf(
        "0.5 0.5 1 rg 0 0 200 100 re f q /M gs 1 1 0 rg 0 0 100 100 re f Q q /S gs 0.5 0 0 rg 100 0 100 100 re f Q",
        "/ExtGState << /M << /BM /Multiply >> /S << /BM [/Screen] >> >>",
        vec![],
    );
    let img = render(&parse(&blend).unwrap());
    let m = img.get(50, 50);
    assert!((m[0] - 0.5).abs() < 0.01 && (m[1] - 0.5).abs() < 0.01 && m[2] < 0.01, "multiply {m:?}");
    let s = img.get(150, 50);
    assert!((s[0] - 0.75).abs() < 0.01 && (s[1] - 0.5).abs() < 0.01 && (s[2] - 1.0).abs() < 0.01, "screen {s:?}");
}

#[test]
fn tiling_patterns_coloured_and_uncoloured() {
    let cell = b"1 0 0 rg 0 0 10 10 re f 0 0 1 rg 10 10 10 10 re f".to_vec();
    let bytes = page_pdf(
        "/Pattern cs /P1 scn 0 0 100 100 re f /Cs1 cs 0 1 0 /P2 scn 100 0 100 100 re f",
        "/Pattern << /P1 10 0 R /P2 11 0 R >> /ColorSpace << /Cs1 [/Pattern /DeviceRGB] >>",
        vec![
            (10, "<< /Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 20 20] /XStep 20 /YStep 20 /Resources << >> >>".into(), Some(cell)),
            (
                11,
                "<< /Type /Pattern /PatternType 1 /PaintType 2 /TilingType 1 /BBox [0 0 20 20] /XStep 20 /YStep 20 >>".into(),
                Some(b"1 0 0 rg 0 0 10 10 re f".to_vec()),
            ),
        ],
    );
    let doc = parse(&bytes).unwrap();
    assert!(doc.skipped.is_empty(), "{:?}", doc.skipped);
    let img = render(&doc);
    // PDF (x, y) → pixel (x, 100 − y).
    let at = |x: i64, y: i64| img.get(x, 100 - y);
    assert!(at(5, 5)[0] > 0.99 && at(25, 5)[0] > 0.99 && at(85, 45)[0] > 0.99, "{:?}", at(5, 5));
    assert!(at(15, 15)[2] > 0.99 && at(55, 35)[2] > 0.99);
    assert_eq!(at(15, 5)[3], 0.0);
    // Uncoloured: the cell in the colour given with the pattern (green), its own colour ignored.
    assert!(at(105, 5)[1] > 0.99 && at(105, 5)[0] < 0.01, "{:?}", at(105, 5));
    assert_eq!(at(115, 15)[3], 0.0);
}

#[test]
fn multiple_pages_and_calculator_shadings() {
    let objs: Objs = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into(), None),
        (2, "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>".into(), None),
        (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".into(), None),
        (4, "<< >>".into(), Some(b"1 0 0 rg 0 0 200 100 re f".to_vec())),
        (5, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 50] /Resources << /Shading << /Sh 7 0 R >> >> /Contents 6 0 R >>".into(), None),
        (6, "<< >>".into(), Some(b"/Sh sh".to_vec())),
        (7, "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 100 0] /Function 8 0 R >>".into(), None),
        (8, "<< /FunctionType 4 /Domain [0 1] /Range [0 1 0 1 0 1] >>".into(), Some(b"{ 0 0 }".to_vec())),
    ];
    let bytes = pdf(&objs, 1);
    assert_eq!(page_count(&bytes), 2);
    let p1 = parse_page(&bytes, 0).unwrap();
    assert_eq!((p1.width, p1.height), (200.0, 100.0));
    let p2 = parse_page(&bytes, 1).unwrap();
    assert_eq!((p2.width, p2.height), (100.0, 50.0));
    assert!(p2.skipped.is_empty(), "{:?}", p2.skipped);
    let img = render(&p2);
    assert!(img.get(5, 25)[0] < 0.1 && img.get(95, 25)[0] > 0.9, "{:?} {:?}", img.get(5, 25), img.get(95, 25));
    assert_eq!(parse_page(&bytes, 2), Err(Error::PageOutOfRange(2)));
}
