use serde_json::json;

use super::*;

fn session_with_frame() -> (Session, u64, u64) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [36, 36, 336, 500], "content": "text"})).unwrap();
    let sid = r["story"].as_u64().unwrap();
    let fid = r["id"].as_u64().unwrap();
    s.execute("text.insert", &json!({"text": "Intro"})).unwrap();
    (s, sid, fid)
}

fn check(s: &Session) {
    s.doc().unwrap().doc.check().unwrap();
}

#[test]
fn insert_type_in_cells_and_tab() {
    let (mut s, sid, _) = session_with_frame();
    let r = s.execute("table.insert", &json!({"rows": 2, "cols": 3, "headerRows": 1})).unwrap();
    let tid = r["table"].as_u64().unwrap();
    check(&s);
    // The caret is in the first cell; typing goes there.
    s.execute("text.insert", &json!({"text": "Name"})).unwrap();
    s.execute("text.insert", &json!({"text": "\t"})).unwrap();
    s.execute("text.insert", &json!({"text": "Value"})).unwrap();
    check(&s);
    let t = s.execute("table.get", &json!({})).unwrap();
    assert_eq!(t["table"], tid);
    assert_eq!(t["headerRows"], 1);
    assert_eq!(t["cells"][0]["text"], "Name");
    assert_eq!(t["cells"][1]["text"], "Value");
    // The story text holds only the anchor.
    let story = s.execute("story.get", &json!({"story": sid})).unwrap();
    assert!(story["text"].as_str().unwrap().contains(designcraft_doc::TABLE_ANCHOR));
    assert!(!story["text"].as_str().unwrap().contains("Name"));
    // Backspace edits the cell.
    s.execute("text.delete", &json!({})).unwrap();
    assert_eq!(s.execute("table.get", &json!({})).unwrap()["cells"][1]["text"], "Valu");
    // Tab in the last cell adds a row.
    for _ in 0..8 {
        s.execute("table.nextCell", &json!({})).unwrap();
    }
    assert_eq!(s.execute("table.get", &json!({})).unwrap()["rows"].as_array().unwrap().len(), 4);
    check(&s);
    // Undo restores the previous shape.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.execute("table.get", &json!({"table": tid})).unwrap()["rows"].as_array().unwrap().len(), 3);
}

#[test]
fn rows_columns_merge_and_options() {
    let (mut s, _, _) = session_with_frame();
    s.execute("table.insert", &json!({"rows": 3, "cols": 3})).unwrap();
    s.execute("table.insertRow", &json!({"where": "below", "count": 2})).unwrap();
    s.execute("table.insertColumnLeft", &json!({})).unwrap();
    check(&s);
    let t = s.execute("table.get", &json!({})).unwrap();
    assert_eq!((t["rows"].as_array().unwrap().len(), t["columns"].as_array().unwrap().len()), (5, 4));
    // The caret's cell moved right with the inserted column.
    assert_eq!(s.doc().unwrap().selection.text.unwrap().cell.unwrap().col, 1);
    s.execute("table.select", &json!({"rows": [1, 2], "cols": [1, 2]})).unwrap();
    s.execute("table.merge", &json!({})).unwrap();
    check(&s);
    let t = s.execute("table.get", &json!({})).unwrap();
    let m = t["cells"].as_array().unwrap().iter().find(|c| c["row"] == 1 && c["col"] == 1).unwrap().clone();
    assert_eq!((m["rowSpan"].as_u64(), m["colSpan"].as_u64()), (Some(2), Some(2)));
    s.execute("table.unmerge", &json!({})).unwrap();
    check(&s);
    s.execute("table.select", &json!({"what": "row", "rows": [0, 0]})).unwrap();
    s.execute("table.setCell", &json!({"fill": "[Black]", "tint": 0.3, "insets": 6, "vj": "center", "stroke": {"weight": 2, "edges": "bottom"}}))
        .unwrap();
    s.execute("table.setRowHeight", &json!({"height": 30, "mode": "exactly"})).unwrap();
    s.execute("table.setColumnWidth", &json!({"cols": [0, 0], "width": 40})).unwrap();
    s.execute(
        "table.options",
        &json!({"headerRows": 1, "altRows": {"first": 1, "firstColor": "[Black]", "firstTint": 0.1, "next": 1}, "border": {"weight": 2}}),
    )
    .unwrap();
    check(&s);
    let t = s.execute("table.get", &json!({})).unwrap();
    assert_eq!(t["headerRows"], 1);
    assert_eq!(t["rows"][0]["mode"], "exactly");
    assert_eq!(t["rows"][0]["height"], 30.0);
    assert_eq!(t["columns"][0]["width"], 40.0);
    assert_eq!(t["cells"][0]["fill"], "[Black]");
    assert_eq!(t["options"]["border"]["weight"], 2.0);
    // Composition reflects the fixed row height.
    let st = s.doc().unwrap();
    let sid = st.selection.text.unwrap().story;
    let cs = s.cache.get(&st.doc, sid, None);
    let tf = &cs.frames[0].tables[0];
    assert!((tf.cell(0, 1).unwrap().rect.height() - 30.0).abs() < 1e-6);
    assert!(tf.cell(1, 0).unwrap().fill.is_some());
    // Delete a row and a column, then the whole table.
    s.execute("table.deleteRow", &json!({"rows": [4, 4]})).unwrap();
    s.execute("table.deleteColumn", &json!({"cols": [0, 0]})).unwrap();
    check(&s);
    let t = s.execute("table.get", &json!({})).unwrap();
    assert_eq!((t["rows"].as_array().unwrap().len(), t["columns"].as_array().unwrap().len()), (4, 3));
    s.execute("table.delete", &json!({})).unwrap();
    check(&s);
    let st = s.doc().unwrap();
    assert!(st.doc.stories.values().all(|x| x.tables.is_empty()));
    assert_eq!(st.doc.story(sid).unwrap().text, "Intro");
}

#[test]
fn convert_text_to_table_and_back() {
    let (mut s, sid, _) = session_with_frame();
    s.execute("story.setText", &json!({"story": sid, "text": "Ink\tCoverage\nPlum\t80%\nSunset\t45%\nAfter"})).unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 25})).unwrap();
    let r = s.execute("table.convertFromText", &json!({})).unwrap();
    assert_eq!(r["rows"], 3);
    check(&s);
    let t = s.execute("table.get", &json!({"table": r["table"]})).unwrap();
    assert_eq!(t["columns"].as_array().unwrap().len(), 2);
    assert_eq!(t["cells"][3]["text"], "80%");
    let story = s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text.clone();
    assert_eq!(story, format!("{}\nAfter", designcraft_doc::TABLE_ANCHOR));
    s.execute("table.convertToText", &json!({"table": r["table"]})).unwrap();
    check(&s);
    assert_eq!(s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text, "Ink\tCoverage\nPlum\t80%\nSunset\t45%\nAfter");
}

#[test]
fn caret_placement_in_cells_and_cell_formatting() {
    let (mut s, sid, fid) = session_with_frame();
    let r = s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap();
    let tid = r["table"].as_u64().unwrap();
    s.execute("table.setCell", &json!({"rows": [1, 1], "cols": [1, 1], "text": "Hello"})).unwrap();
    // Click inside cell (1,1): spread coordinates = frame inner space here.
    let (rect, _) = {
        let st = s.doc().unwrap();
        let cs = s.cache.get(&st.doc, designcraft_doc::StoryId(sid), None);
        let c = cs.frames[0].tables[0].cell(1, 1).unwrap().clone();
        (c.rect, c)
    };
    let r = s.execute("text.placeCaret", &json!({"frame": fid, "point": [rect.x1 - 1.0, rect.center().y]})).unwrap();
    assert_eq!(r["cell"]["row"], 1);
    assert_eq!(r["cell"]["col"], 1);
    assert_eq!(r["pos"], 5);
    s.execute("text.insert", &json!({"text": "!"})).unwrap();
    assert_eq!(s.execute("table.get", &json!({"table": tid})).unwrap()["cells"][3]["text"], "Hello!");
    // Formatting at a cell caret/selection goes to the cell story.
    s.execute("edit.selectAll", &json!({})).unwrap();
    s.execute("type.char", &json!({"attrs": {"size": 20}})).unwrap();
    assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["size"], 20.0);
    check(&s);
    // Selected cells format whole cells.
    s.execute("table.select", &json!({"table": tid, "what": "table"})).unwrap();
    s.execute("type.para", &json!({"attrs": {"align": "center"}})).unwrap();
    let st = s.doc().unwrap();
    let t = &st.doc.story(designcraft_doc::StoryId(sid)).unwrap().tables[&tid];
    assert!(t.cells.iter().all(|c| c.text.paras[0].para.align == Some(designcraft_doc::Align::Center)));
    // Typing after the table starts a new paragraph instead of hiding text in the anchor paragraph.
    let a = st.doc.story(designcraft_doc::StoryId(sid)).unwrap().table_anchor(tid).unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": a + 3, "focus": a + 3})).unwrap();
    s.execute("text.insert", &json!({"text": "Tail"})).unwrap();
    check(&s);
    let text = s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text.clone();
    assert!(text.contains(&format!("{}\nTail", designcraft_doc::TABLE_ANCHOR)), "{text:?}");
}

#[test]
fn file_roundtrip_keeps_tables() {
    let (mut s, sid, _) = session_with_frame();
    s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap();
    s.execute("text.insert", &json!({"text": "A1"})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let bytes = crate::cmd::file_bytes(&d);
    let back = crate::cmd::file_from(&bytes).unwrap();
    back.check().unwrap();
    let st = back.story(designcraft_doc::StoryId(sid)).unwrap();
    assert_eq!(st.tables.values().next().unwrap().cells[0].text.text, "A1");
}
