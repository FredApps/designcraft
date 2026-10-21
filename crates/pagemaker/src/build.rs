//! A [`PmDoc`] as a DesignCraft [`Document`]: pages, master pages (the A-Parent), threaded text
//! frames with their stories' formatting, lines, boxes, ovals and frames for placed graphics.

use std::collections::BTreeMap;

use designcraft_doc::build::NewDocument;
use designcraft_doc::{Align as DcAlign, Document, Fill, Item, ItemId, Leading, Margins, ParaFormat, Shape, SpreadRef, Stroke};
use designcraft_geom::{Affine, Point, Rect, Unit, shapes};

use crate::PmError;
use crate::model::{Align, ObjKind, PmDoc, PmStory};

/// The document, and what didn't come across.
pub fn build(pm: &PmDoc) -> Result<(Document, Vec<String>), PmError> {
    let mut warnings = pm.warnings.clone();
    let margins = pm.margins.map_or(Margins::uniform(36.0), |[top, bottom, inside, outside]| Margins { top, bottom, inside, outside });
    let nd = NewDocument {
        width: pm.width,
        height: pm.height,
        pages: pm.pages.len().max(1),
        facing_pages: pm.facing,
        margins,
        units: Unit::Inches,
        ..NewDocument::default()
    };
    let mut d = Document::new(&nd);
    let layer = d.default_layer();
    // Text blocks of each story, in block order: (block, spread, rect in spread space).
    let mut blocks: BTreeMap<u32, Vec<(u32, SpreadRef, Rect, Affine)>> = BTreeMap::new();
    let mut graphics = 0usize;
    let mut shapes_kept = 0usize;

    let mut place = |d: &mut Document, r: SpreadRef, origin: (f64, f64), obj: &crate::model::Obj| -> Result<(), PmError> {
        let [x0, y0, x1, y1] = obj.rect;
        let bounds = Rect::new(origin.0 + x0, origin.1 + y0, origin.0 + x1, origin.1 + y1);
        // A rotated object: its unrotated shape about the bounds' centre, then turned.
        let (rect, xf) = match obj.rotation {
            Some((deg, w, h)) => {
                let c = bounds.center();
                (Rect::new(c.x - w / 2.0, c.y - h / 2.0, c.x + w / 2.0, c.y + h / 2.0), Affine::rotate_about(deg.to_radians(), c))
            }
            None => (bounds, Affine::IDENTITY),
        };
        let (shape, path, stroke) = match obj.kind {
            ObjKind::Text { story, block } => {
                blocks.entry(story).or_default().push((block, r, rect, xf));
                return Ok(());
            }
            ObjKind::Line => {
                let (a, b) = if rect.width() >= rect.height() {
                    let y = (rect.y0 + rect.y1) / 2.0;
                    (Point::new(rect.x0, y), Point::new(rect.x1, y))
                } else {
                    let x = (rect.x0 + rect.x1) / 2.0;
                    (Point::new(x, rect.y0), Point::new(x, rect.y1))
                };
                (Shape::GraphicLine, shapes::line(a, b), Stroke::default())
            }
            ObjKind::Box => (Shape::Rectangle, shapes::rectangle(rect), Stroke::default()),
            ObjKind::Oval => (Shape::Oval, shapes::ellipse(rect), Stroke::default()),
            ObjKind::Graphic => {
                graphics += 1;
                (Shape::Rectangle, shapes::rectangle(rect), Stroke::none())
            }
            ObjKind::Shape => {
                shapes_kept += 1;
                (Shape::Rectangle, shapes::rectangle(rect), Stroke::default())
            }
        };
        let mut it = Item::new(ItemId(d.alloc()), layer, shape, path);
        it.xf = xf;
        it.stroke = stroke;
        it.fill = Fill::none();
        if matches!(obj.kind, ObjKind::Graphic) {
            it.name = "PageMaker graphic".into();
        }
        d.insert_item(r, it, None).map_err(|e| PmError::Corrupt(e.to_string()))?;
        Ok(())
    };

    // PageMaker measures x from the spine (facing pages) or the page's centre: where that is on
    // a spread.
    let origin = |pages: &[designcraft_doc::Page]| -> Option<(f64, f64)> {
        let first = pages.first()?.bounds();
        let x = if !pm.facing {
            (first.x0 + first.x1) / 2.0
        } else {
            match pages.iter().find(|p| p.side == designcraft_doc::PageSide::Right) {
                Some(right) => right.bounds().x0,
                None => first.x1,
            }
        };
        Some((x, first.y0))
    };
    // The master spread is the A-Parent.
    if let Some(o) = d.parents.first().and_then(|p| origin(&p.pages)) {
        for obj in &pm.masters {
            place(&mut d, SpreadRef::Parent(0), o, obj)?;
        }
    }
    for (i, objs) in pm.pages.iter().enumerate() {
        let Some((si, _)) = d.page_loc(i) else { continue };
        let Some(o) = d.spreads.get(si).and_then(|s| origin(&s.pages)) else { continue };
        for obj in objs {
            place(&mut d, SpreadRef::Doc(si), o, obj)?;
        }
    }

    // Each story flows through its blocks, threaded in block order.
    let mut missing = 0usize;
    for (story, mut frames) in blocks {
        frames.sort_by_key(|f| f.0);
        let Some(st) = pm.stories.get(&story) else {
            missing += 1;
            continue;
        };
        let mut prev: Option<ItemId> = None;
        let mut sid = None;
        for (k, (_, r, rect, xf)) in frames.iter().enumerate() {
            let text = if k == 0 { story_text(st) } else { String::new() };
            let (id, s) = d.add_text_frame(*r, *rect, layer, &text, ParaFormat::default()).map_err(|e| PmError::Corrupt(e.to_string()))?;
            if let Some(it) = d.item_mut(id) {
                it.xf = *xf;
            }
            match prev {
                Some(p) => d.thread(p, id).map_err(|e| PmError::Corrupt(e.to_string()))?,
                None => sid = Some(s),
            }
            prev = Some(id);
        }
        if let Some(story) = sid.and_then(|s| d.story_mut(s)) {
            format_story(story, st);
        }
    }
    if missing > 0 {
        warnings.push(format!("{missing} text block(s) name a story the file doesn't hold; they were left out"));
    }
    if graphics > 0 {
        warnings.push(format!("{graphics} placed graphic(s) came in as empty frames: their images aren't imported yet"));
    }
    if shapes_kept > 0 {
        warnings.push(format!("{shapes_kept} polygon(s) or other drawn shape(s) came in as rectangles of their bounds"));
    }
    warnings.extend(d.fix_parent_bases());
    d.check().map_err(|e| PmError::Corrupt(e.to_string()))?;
    Ok((d, warnings))
}

/// The story's text without the last paragraph mark (a frame's story ends without one).
fn story_text(st: &PmStory) -> String {
    st.text.strip_suffix('\n').unwrap_or(&st.text).to_string()
}

/// Apply the character and paragraph runs. Run lengths count characters of the file's text.
fn format_story(story: &mut designcraft_doc::Story, st: &PmStory) {
    // Byte offset of each character of the story text, and its end.
    let len = story.text.len();
    let mut at: Vec<usize> = story.text.char_indices().map(|(i, _)| i).collect();
    at.push(len);
    let byte = |c: usize| at.get(c).copied().unwrap_or(len);
    let mut pos = 0usize;
    let mut ranges = Vec::new();
    for run in &st.chars {
        let end = pos.saturating_add(run.len);
        ranges.push((byte(pos), byte(end), run.clone()));
        pos = end;
    }
    for (a, b, run) in ranges {
        if a >= b {
            continue;
        }
        story.format_chars(a..b, |cf| {
            let o = &mut cf.over;
            if let Some(f) = &run.font {
                o.font_family = Some(f.clone());
            }
            o.font_style = Some(
                match (run.bold, run.italic) {
                    (true, true) => "Bold Italic",
                    (true, false) => "Bold",
                    (false, true) => "Italic",
                    (false, false) => "Regular",
                }
                .into(),
            );
            if let Some(s) = run.size {
                o.size = Some(s);
            }
            o.leading = Some(run.leading.map_or(Leading::Auto, Leading::Points));
            if run.underline {
                o.underline = Some(true);
            }
        });
    }
    let mut pos = 0usize;
    let mut ranges = Vec::new();
    for run in &st.paras {
        let end = pos.saturating_add(run.len);
        ranges.push((byte(pos), byte(end.saturating_sub(1)).max(byte(pos)), run.align));
        pos = end;
    }
    for (a, b, align) in ranges {
        story.format_paras(a..b, |pf| {
            pf.para.align = Some(match align {
                Align::Left => DcAlign::Left,
                Align::Center => DcAlign::Center,
                Align::Right => DcAlign::Right,
                Align::Justify => DcAlign::LeftJustified,
            });
        });
    }
}
