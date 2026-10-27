//! Slide shows of pictures, such as maps and charts made in a model: PowerPoint (.pptx)
//! and LibreOffice Impress (.odp), a slide for each picture with its title above it, and a
//! first slide with the title of the whole if one is given. Written by hand as the XML the
//! two formats need, packed in a zip file.

use crate::model::{GbError, RasterImage, Result};
use std::io::Write;

/// One slide: a title and a picture.
pub struct Slide<'a> {
    pub title: String,
    pub picture: &'a RasterImage,
}

/// How the slides look.
pub struct Deck {
    /// The title of the first slide, or empty for none.
    pub cover: String,
    /// A line under it, such as a date or who made it.
    pub cover_note: String,
    /// 16:9, or else 4:3.
    pub wide: bool,
    /// Each slide's title above its picture.
    pub titles: bool,
    /// The colour behind the pictures, as #rrggbb, or None for white.
    pub background: Option<[u8; 3]>,
}

const EMU_PER_INCH: i64 = 914_400;

/// The size of a slide, in EMU (914 400 to an inch).
fn slide_size(wide: bool) -> (i64, i64) {
    if wide {
        (12_192_000, 6_858_000)
    } else {
        (9_144_000, 6_858_000)
    }
}

/// Where a picture goes on a slide, as large as fits below the title, in the middle:
/// (x, y, width, height) in EMU.
fn place(pic_w: usize, pic_h: usize, slide: (i64, i64), title: bool) -> (i64, i64, i64, i64) {
    let margin = EMU_PER_INCH * 3 / 10;
    let top = if title { EMU_PER_INCH * 11 / 10 } else { margin };
    let (aw, ah) = (slide.0 - 2 * margin, slide.1 - top - margin);
    let k = (aw as f64 / pic_w.max(1) as f64).min(ah as f64 / pic_h.max(1) as f64);
    let (w, h) = ((pic_w as f64 * k) as i64, (pic_h as f64 * k) as i64);
    (margin + (aw - w) / 2, top + (ah - h) / 2, w, h)
}

fn png(img: &RasterImage) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let enc = image::codecs::png::PngEncoder::new(&mut out);
        image::ImageEncoder::write_image(enc, &img.rgba, img.width as u32, img.height as u32, image::ExtendedColorType::Rgba8)
            .map_err(|e| GbError(format!("Cannot make the picture for the slides: {}", e)))?;
    }
    Ok(out)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn hex(c: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

struct Zip<W: Write + std::io::Seek> {
    zip: zip::ZipWriter<W>,
}

impl<W: Write + std::io::Seek> Zip<W> {
    fn put(&mut self, name: &str, data: &[u8], stored: bool) -> Result<()> {
        let opts =
            zip::write::SimpleFileOptions::default().compression_method(if stored { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated });
        self.zip.start_file(name, opts).map_err(|e| GbError(format!("Cannot write the slides: {}", e)))?;
        self.zip.write_all(data).map_err(|e| GbError(format!("Cannot write the slides: {}", e)))?;
        Ok(())
    }
}

fn open(path: &std::path::Path) -> Result<Zip<std::fs::File>> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| GbError(format!("Cannot create the folder {}: {}", dir.display(), e)))?;
    }
    let f = std::fs::File::create(path).map_err(|e| GbError(format!("Cannot write {}: {}", path.display(), e)))?;
    Ok(Zip { zip: zip::ZipWriter::new(f) })
}

// ---------- PowerPoint ----------

const THEME: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Ographis"><a:themeElements><a:clrScheme name="Ographis"><a:dk1><a:srgbClr val="1F1F1F"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="333F4F"/></a:dk2><a:lt2><a:srgbClr val="EEECE1"/></a:lt2><a:accent1><a:srgbClr val="2A78D6"/></a:accent1><a:accent2><a:srgbClr val="EB6834"/></a:accent2><a:accent3><a:srgbClr val="1BAF7A"/></a:accent3><a:accent4><a:srgbClr val="EDA100"/></a:accent4><a:accent5><a:srgbClr val="6250D6"/></a:accent5><a:accent6><a:srgbClr val="E34948"/></a:accent6><a:hlink><a:srgbClr val="2A78D6"/></a:hlink><a:folHlink><a:srgbClr val="6250D6"/></a:folHlink></a:clrScheme><a:fontScheme name="Ographis"><a:majorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Ographis"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="25400"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="38100"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"#;

const NS_P: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;

#[allow(clippy::too_many_arguments)]
fn text_box(id: u32, name: &str, x: i64, y: i64, w: i64, h: i64, text: &str, size: u32, bold: bool, centre: bool, colour: &str) -> String {
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{w}" cy="{h}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="square" anchor="ctr"><a:normAutofit/></a:bodyPr><a:lstStyle/><a:p><a:pPr algn="{algn}"/><a:r><a:rPr lang="sv-SE" sz="{sz}" b="{b}" dirty="0"><a:solidFill><a:srgbClr val="{colour}"/></a:solidFill></a:rPr><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>"#,
        id = id,
        name = name,
        x = x,
        y = y,
        w = w,
        h = h,
        algn = if centre { "ctr" } else { "l" },
        sz = size * 100,
        b = if bold { 1 } else { 0 },
        colour = colour,
        text = esc(text)
    )
}

fn background(deck: &Deck) -> String {
    match deck.background {
        Some(c) => format!(r#"<p:bg><p:bgPr><a:solidFill><a:srgbClr val="{}"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>"#, hex(c)),
        None => String::new(),
    }
}

/// Dark text on a light background, light on a dark one.
fn ink(deck: &Deck) -> &'static str {
    match deck.background {
        Some(c) if (c[0] as u32 * 299 + c[1] as u32 * 587 + c[2] as u32 * 114) / 1000 < 128 => "F5F5F5",
        _ => "1F1F1F",
    }
}

pub fn write_pptx(path: &std::path::Path, slides: &[Slide], deck: &Deck) -> Result<()> {
    let (sw, sh) = slide_size(deck.wide);
    let mut z = open(path)?;
    let cover = !deck.cover.trim().is_empty();
    let n = slides.len() + cover as usize;
    let mut types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>"#,
    );
    for k in 1..=n {
        types.push_str(&format!(
            r#"<Override PartName="/ppt/slides/slide{}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#,
            k
        ));
    }
    types.push_str("</Types>");
    z.put("[Content_Types].xml", types.as_bytes(), false)?;
    z.put(
        "_rels/.rels",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#,
        false,
    )?;
    let title = if cover { deck.cover.clone() } else { slides.first().map(|s| s.title.clone()).unwrap_or_default() };
    z.put(
        "docProps/core.xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>{}</dc:title><dc:creator>Ographis</dc:creator></cp:coreProperties>"#,
            esc(&title)
        )
        .as_bytes(),
        false,
    )?;
    z.put(
        "docProps/app.xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Ographis</Application><Slides>{}</Slides></Properties>"#,
            n
        )
        .as_bytes(),
        false,
    )?;
    let mut list = String::new();
    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/>"#,
    );
    for k in 1..=n {
        list.push_str(&format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 255 + k, k + 10));
        rels.push_str(&format!(
            r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{}.xml"/>"#,
            k + 10,
            k
        ));
    }
    rels.push_str("</Relationships>");
    z.put(
        "ppt/presentation.xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation {ns} saveSubsetFonts="1"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>{list}</p:sldIdLst><p:sldSz cx="{sw}" cy="{sh}"/><p:notesSz cx="6858000" cy="9144000"/><p:defaultTextStyle><a:defPPr><a:defRPr lang="sv-SE"/></a:defPPr></p:defaultTextStyle></p:presentation>"#,
            ns = NS_P,
            list = list,
            sw = sw,
            sh = sh
        )
        .as_bytes(),
        false,
    )?;
    z.put("ppt/_rels/presentation.xml.rels", rels.as_bytes(), false)?;
    z.put("ppt/theme/theme1.xml", THEME.as_bytes(), false)?;
    let tree = r#"<p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld>"#;
    z.put(
        "ppt/slideMasters/slideMaster1.xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster {ns}>{tree}<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle><a:lvl1pPr><a:defRPr sz="3200"/></a:lvl1pPr></p:titleStyle><p:bodyStyle><a:lvl1pPr><a:defRPr sz="1800"/></a:lvl1pPr></p:bodyStyle><p:otherStyle><a:lvl1pPr><a:defRPr sz="1800"/></a:lvl1pPr></p:otherStyle></p:txStyles></p:sldMaster>"#,
            ns = NS_P,
            tree = tree
        )
        .as_bytes(),
        false,
    )?;
    z.put(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/></Relationships>"#,
        false,
    )?;
    z.put(
        "ppt/slideLayouts/slideLayout1.xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout {ns} type="blank" preserve="1">{tree}<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#,
            ns = NS_P,
            tree = tree.replace("<p:cSld>", r#"<p:cSld name="Blank">"#)
        )
        .as_bytes(),
        false,
    )?;
    z.put(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#,
        false,
    )?;
    let group = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#;
    let layout_rel = r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>"#;
    let colour = ink(deck);
    let mut k = 0;
    if cover {
        k += 1;
        let mut shapes =
            text_box(2, "Title", EMU_PER_INCH / 2, sh / 2 - EMU_PER_INCH, sw - EMU_PER_INCH, EMU_PER_INCH * 13 / 10, &deck.cover, 40, true, true, colour);
        if !deck.cover_note.trim().is_empty() {
            shapes.push_str(&text_box(
                3,
                "Note",
                EMU_PER_INCH / 2,
                sh / 2 + EMU_PER_INCH * 4 / 10,
                sw - EMU_PER_INCH,
                EMU_PER_INCH * 7 / 10,
                &deck.cover_note,
                20,
                false,
                true,
                colour,
            ));
        }
        z.put(
            &format!("ppt/slides/slide{}.xml", k),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld {ns}><p:cSld>{bg}<p:spTree>{group}{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#,
                ns = NS_P,
                bg = background(deck),
                group = group,
                shapes = shapes
            )
            .as_bytes(),
            false,
        )?;
        z.put(
            &format!("ppt/slides/_rels/slide{}.xml.rels", k),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{}</Relationships>"#,
                layout_rel
            )
            .as_bytes(),
            false,
        )?;
    }
    for (i, s) in slides.iter().enumerate() {
        k += 1;
        let titled = deck.titles && !s.title.trim().is_empty();
        let (x, y, w, h) = place(s.picture.width, s.picture.height, (sw, sh), titled);
        let mut shapes = String::new();
        if titled {
            shapes.push_str(&text_box(
                2,
                "Title",
                EMU_PER_INCH * 3 / 10,
                EMU_PER_INCH / 5,
                sw - EMU_PER_INCH * 6 / 10,
                EMU_PER_INCH * 8 / 10,
                &s.title,
                28,
                true,
                false,
                colour,
            ));
        }
        shapes.push_str(&format!(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="3" name="Picture {i}" descr="{d}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId2"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{w}" cy="{h}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#,
            i = i + 1,
            d = esc(&s.title),
            x = x,
            y = y,
            w = w,
            h = h
        ));
        z.put(
            &format!("ppt/slides/slide{}.xml", k),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld {ns}><p:cSld>{bg}<p:spTree>{group}{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#,
                ns = NS_P,
                bg = background(deck),
                group = group,
                shapes = shapes
            )
            .as_bytes(),
            false,
        )?;
        z.put(
            &format!("ppt/slides/_rels/slide{}.xml.rels", k),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{}<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image{}.png"/></Relationships>"#,
                layout_rel,
                i + 1
            )
            .as_bytes(),
            false,
        )?;
        z.put(&format!("ppt/media/image{}.png", i + 1), &png(s.picture)?, true)?;
    }
    z.zip.finish().map_err(|e| GbError(format!("Cannot write the slides: {}", e)))?;
    Ok(())
}

// ---------- Impress ----------

/// A length in EMU as centimetres, as OpenDocument writes it.
fn cm(emu: i64) -> String {
    format!("{:.3}cm", emu as f64 / 360_000.0)
}

pub fn write_odp(path: &std::path::Path, slides: &[Slide], deck: &Deck) -> Result<()> {
    let (sw, sh) = slide_size(deck.wide);
    let mut z = open(path)?;
    // The type first and not packed, as the format asks.
    z.put("mimetype", b"application/vnd.oasis.opendocument.presentation", true)?;
    let mut manifest = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3"><manifest:file-entry manifest:full-path="/" manifest:version="1.3" manifest:media-type="application/vnd.oasis.opendocument.presentation"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/>"#,
    );
    for i in 0..slides.len() {
        manifest.push_str(&format!(r#"<manifest:file-entry manifest:full-path="Pictures/image{}.png" manifest:media-type="image/png"/>"#, i + 1));
    }
    manifest.push_str("</manifest:manifest>");
    let ns = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0""#;
    let fill = match deck.background {
        Some(c) => format!(r##" draw:fill="solid" draw:fill-color="#{}""##, hex(c).to_lowercase()),
        None => r##" draw:fill="solid" draw:fill-color="#ffffff""##.to_string(),
    };
    let ink = format!("#{}", ink(deck).to_lowercase());
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-styles {ns} office:version="1.3"><office:styles><style:style style:name="standard" style:family="graphic"><style:graphic-properties draw:stroke="none" draw:fill="none"/></style:style></office:styles><office:automatic-styles><style:page-layout style:name="PM1"><style:page-layout-properties fo:margin-top="0cm" fo:margin-bottom="0cm" fo:margin-left="0cm" fo:margin-right="0cm" fo:page-width="{w}" fo:page-height="{h}" style:print-orientation="landscape"/></style:page-layout><style:style style:name="Mdp1" style:family="drawing-page"><style:drawing-page-properties{fill}/></style:style></office:automatic-styles><office:master-styles><style:master-page style:name="Default" style:page-layout-name="PM1" draw:style-name="Mdp1"/></office:master-styles></office:document-styles>"#,
        ns = ns,
        w = cm(sw),
        h = cm(sh),
        fill = fill
    );
    let title = if !deck.cover.trim().is_empty() { deck.cover.clone() } else { slides.first().map(|s| s.title.clone()).unwrap_or_default() };
    let meta = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-meta {ns} office:version="1.3"><office:meta><dc:title>{t}</dc:title><meta:generator>Ographis</meta:generator></office:meta></office:document-meta>"#,
        ns = ns,
        t = esc(&title)
    );
    let text_frame = |x: i64, y: i64, w: i64, h: i64, text: &str, style: &str| {
        format!(
            r#"<draw:frame draw:style-name="gr1" svg:x="{x}" svg:y="{y}" svg:width="{w}" svg:height="{h}"><draw:text-box><text:p text:style-name="{style}"><text:span text:style-name="{style}s">{t}</text:span></text:p></draw:text-box></draw:frame>"#,
            x = cm(x),
            y = cm(y),
            w = cm(w),
            h = cm(h),
            style = style,
            t = esc(text)
        )
    };
    let mut pages = String::new();
    let mut n = 0;
    if !deck.cover.trim().is_empty() {
        n += 1;
        pages.push_str(&format!(r#"<draw:page draw:name="page{}" draw:master-page-name="Default" draw:style-name="dp1">"#, n));
        pages.push_str(&text_frame(EMU_PER_INCH / 2, sh / 2 - EMU_PER_INCH, sw - EMU_PER_INCH, EMU_PER_INCH * 13 / 10, &deck.cover, "P1"));
        if !deck.cover_note.trim().is_empty() {
            pages.push_str(&text_frame(EMU_PER_INCH / 2, sh / 2 + EMU_PER_INCH * 4 / 10, sw - EMU_PER_INCH, EMU_PER_INCH * 7 / 10, &deck.cover_note, "P2"));
        }
        pages.push_str("</draw:page>");
    }
    for (i, s) in slides.iter().enumerate() {
        n += 1;
        let titled = deck.titles && !s.title.trim().is_empty();
        let (x, y, w, h) = place(s.picture.width, s.picture.height, (sw, sh), titled);
        pages.push_str(&format!(r#"<draw:page draw:name="page{}" draw:master-page-name="Default" draw:style-name="dp1">"#, n));
        if titled {
            pages.push_str(&text_frame(EMU_PER_INCH * 3 / 10, EMU_PER_INCH / 5, sw - EMU_PER_INCH * 6 / 10, EMU_PER_INCH * 8 / 10, &s.title, "P3"));
        }
        pages.push_str(&format!(
            r#"<draw:frame draw:style-name="gr1" svg:x="{x}" svg:y="{y}" svg:width="{w}" svg:height="{h}"><draw:image xlink:href="Pictures/image{k}.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/><svg:desc>{d}</svg:desc></draw:frame>"#,
            x = cm(x),
            y = cm(y),
            w = cm(w),
            h = cm(h),
            k = i + 1,
            d = esc(&s.title)
        ));
        pages.push_str("</draw:page>");
    }
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content {ns} office:version="1.3"><office:automatic-styles><style:style style:name="dp1" style:family="drawing-page"><style:drawing-page-properties{fill}/></style:style><style:style style:name="gr1" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:stroke="none" draw:fill="none" draw:textarea-vertical-align="middle"/></style:style><style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style><style:style style:name="P1s" style:family="text"><style:text-properties fo:font-family="Arial" fo:font-size="40pt" fo:font-weight="bold" fo:color="{ink}"/></style:style><style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style><style:style style:name="P2s" style:family="text"><style:text-properties fo:font-family="Arial" fo:font-size="20pt" fo:color="{ink}"/></style:style><style:style style:name="P3" style:family="paragraph"><style:paragraph-properties fo:text-align="start"/></style:style><style:style style:name="P3s" style:family="text"><style:text-properties fo:font-family="Arial" fo:font-size="28pt" fo:font-weight="bold" fo:color="{ink}"/></style:style></office:automatic-styles><office:body><office:presentation>{pages}</office:presentation></office:body></office:document-content>"#,
        ns = ns,
        fill = fill,
        ink = ink,
        pages = pages
    );
    z.put("META-INF/manifest.xml", manifest.as_bytes(), false)?;
    z.put("styles.xml", styles.as_bytes(), false)?;
    z.put("meta.xml", meta.as_bytes(), false)?;
    z.put("content.xml", content.as_bytes(), false)?;
    for (i, s) in slides.iter().enumerate() {
        z.put(&format!("Pictures/image{}.png", i + 1), &png(s.picture)?, true)?;
    }
    z.zip.finish().map_err(|e| GbError(format!("Cannot write the slides: {}", e)))?;
    Ok(())
}
