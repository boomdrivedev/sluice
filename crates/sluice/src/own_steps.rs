//! The repository's own steps in a file sluice writes (timber 135; record
//! 0119 as amended 2026-11-18; design D3 of the change
//! `a-pools-run-job-runs-the-repositorys-own-steps`). A file sluice writes
//! may leave rooms, each between a pair of lines that open and close it,
//! whose lines are the repository's own and which sluice writes empty. A
//! file is read against what sluice writes it as twice: by its text, each
//! room's lines taken out and the rest compared line for line; and by its
//! parse as YAML, each room's entries taken out of the list they join, the
//! two parses equal, so nothing a room holds reaches beyond that list. A
//! file is also written again with each room's lines carried over, by a
//! key its caller gives each room.
//!
//! It names no tool and no forge (rules core.1, core.2): each profile
//! gives its own pair of lines, its own keys and its own paths.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use yaml_rust2::{Yaml, YamlLoader};

/// One step of a path into a parsed file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seg {
    /// The value a mapping holds under this key.
    Key(String),
    /// A string, read as YAML of its own.
    Text,
}

/// A path of keys alone.
pub fn keys(path: &[&str]) -> Vec<Seg> {
    path.iter().map(|k| Seg::Key((*k).to_string())).collect()
}

/// What a caller says of the rooms in the files it writes.
pub struct Layout<'a> {
    /// The line that opens a room, as it reads trimmed.
    pub open: &'a str,
    /// The line that closes it.
    pub close: &'a str,
    /// The key of the room, or of the further line, at `at` among a file's
    /// `lines`: what a room is carried by.
    pub key: &'a dyn Fn(&[&str], usize) -> String,
    /// Where, in the parsed file, the entries of the room keyed so join a
    /// list.
    pub list: &'a dyn Fn(&str) -> Vec<Seg>,
    /// One further line the caller names as the repository's.
    pub own_line: Option<OwnLine<'a>>,
}

/// One further line a caller names as the repository's: taken out with
/// the rooms where it stands directly after `after`, at its indentation,
/// in the part of the file keyed `key`, and accepted by the parse in the
/// mapping at `at` alone.
pub struct OwnLine<'a> {
    pub line: &'a str,
    pub after: &'a str,
    pub key: &'a str,
    pub at: Vec<Seg>,
}

/// How a file reads against what sluice writes it as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reading {
    /// As sluice writes it: `rooms` names each room that holds lines of
    /// the repository's, and `own_line` whether the further line stands.
    AsWritten { rooms: Vec<String>, own_line: bool },
    /// It differs outside its rooms.
    Differs(Differs),
    /// A room's lines reach beyond the list its entries join.
    Leaves(Leaves),
}

impl Reading {
    /// Whether the file holds anything of the repository's own.
    pub fn holds_own(&self) -> bool {
        matches!(self, Reading::AsWritten { rooms, own_line } if !rooms.is_empty() || *own_line)
    }
}

/// The first line a file differs at from what sluice writes, by number,
/// what it holds there and what sluice writes there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Differs {
    pub line: usize,
    pub had: Option<String>,
    pub wants: Option<String>,
}

impl std::fmt::Display for Differs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let shown = |l: &Option<String>| match l {
            Some(l) => format!("{:?}", l.trim()),
            None => "nothing".into(),
        };
        write!(
            f,
            "line {} holds {}, where sluice writes {}",
            self.line,
            shown(&self.had),
            shown(&self.wants)
        )
    }
}

/// The first line of a room that reaches beyond its place: the room's
/// key, the line's number and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leaves {
    pub key: String,
    pub line: usize,
    pub text: String,
}

impl std::fmt::Display for Leaves {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}, {:?}", self.line, self.text.trim())
    }
}

/// What a file is, line by line: sluice's own lines, a room's, or the
/// further line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tag {
    Kept,
    Room(usize),
    Own,
}

/// A file split into its lines, each tagged, and its rooms.
struct Split<'t> {
    lines: Vec<&'t str>,
    tags: Vec<Tag>,
    /// Each room: its key and the indices of its opening line and lines.
    rooms: Vec<(String, usize, Vec<usize>)>,
}

impl<'t> Split<'t> {
    fn of(text: &'t str, layout: &Layout<'_>) -> Split<'t> {
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let plain: Vec<&str> = lines
            .iter()
            .map(|l| l.trim_end_matches(['\n', '\r']))
            .collect();
        let trimmed = |i: usize| plain[i].trim();
        let indent = |i: usize| plain[i].len() - plain[i].trim_start().len();
        let mut tags = vec![Tag::Kept; lines.len()];
        let mut rooms = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            if trimmed(i) == layout.open {
                let next = (i + 1..lines.len())
                    .find(|&j| trimmed(j) == layout.open || trimmed(j) == layout.close);
                if let Some(j) = next.filter(|&j| trimmed(j) == layout.close) {
                    let r = rooms.len();
                    for tag in &mut tags[i + 1..j] {
                        *tag = Tag::Room(r);
                    }
                    rooms.push(((layout.key)(&plain, i), i, (i + 1..j).collect()));
                    i = j + 1;
                    continue;
                }
            }
            if let Some(own) = &layout.own_line {
                if i > 0
                    && tags[i - 1] == Tag::Kept
                    && trimmed(i) == own.line
                    && trimmed(i - 1) == own.after
                    && indent(i) == indent(i - 1)
                    && (layout.key)(&plain, i) == own.key
                {
                    tags[i] = Tag::Own;
                }
            }
            i += 1;
        }
        Split { lines, tags, rooms }
    }

    /// The file with the lines `keep` keeps.
    fn assemble(&self, keep: impl Fn(usize, Tag) -> bool) -> String {
        self.lines
            .iter()
            .zip(&self.tags)
            .enumerate()
            .filter(|(i, (_, t))| keep(*i, **t))
            .map(|(_, (l, _))| *l)
            .collect()
    }

    /// Its lines that `keep` keeps, each with its number.
    fn numbered(&self, keep: impl Fn(Tag) -> bool) -> Vec<(usize, &'t str)> {
        self.lines
            .iter()
            .zip(&self.tags)
            .enumerate()
            .filter(|(_, (_, t))| keep(**t))
            .map(|(i, (l, _))| (i + 1, *l))
            .collect()
    }
}

/// The first line `had`'s lines differ at from `wants`'.
fn first_difference(
    had: &[(usize, &str)],
    wants: &[(usize, &str)],
    ends: usize,
) -> Option<Differs> {
    let n = (0..had.len().max(wants.len()))
        .find(|&i| had.get(i).map(|(_, l)| *l) != wants.get(i).map(|(_, l)| *l))?;
    let plain = |l: &str| l.trim_end_matches(['\n', '\r']).to_string();
    Some(Differs {
        line: had.get(n).map_or(ends + 1, |(at, _)| *at),
        had: had.get(n).map(|(_, l)| plain(l)),
        wants: wants.get(n).map(|(_, l)| plain(l)),
    })
}

/// The node at `path` in `y`, each [`Seg::Text`] on the way read as YAML
/// in place.
fn walk<'y>(mut y: &'y mut Yaml, path: &[Seg]) -> Option<&'y mut Yaml> {
    for seg in path {
        match seg {
            Seg::Key(k) => {
                y = match y {
                    Yaml::Hash(h) => h.get_mut(&Yaml::String(k.clone()))?,
                    _ => return None,
                }
            }
            Seg::Text => {
                let parsed = match &*y {
                    Yaml::String(s) => {
                        let mut docs = YamlLoader::load_from_str(s).ok()?;
                        if docs.len() != 1 {
                            return None;
                        }
                        Some(docs.remove(0))
                    }
                    Yaml::Hash(_) | Yaml::Array(_) => None,
                    _ => return None,
                };
                if let Some(parsed) = parsed {
                    *y = parsed;
                }
            }
        }
    }
    Some(y)
}

/// `f` with `s`'s entries in order and the others in one run between them
/// taken out, where it holds them so.
fn splice(f: &mut Vec<Yaml>, s: &[Yaml]) -> bool {
    let Some(k) = f.len().checked_sub(s.len()) else {
        return false;
    };
    for p in 0..=s.len() {
        if f[..p] != s[..p] {
            break;
        }
        if f[p + k..] == s[p..] {
            f.drain(p..p + k);
            return true;
        }
    }
    false
}

/// What sluice writes, parsed, each path its rooms and further line name
/// read through.
struct Wants {
    doc: Yaml,
    keys: Vec<String>,
}

impl Wants {
    fn of(wants: &str, layout: &Layout<'_>, keys: Vec<String>) -> Result<Wants> {
        let mut docs = YamlLoader::load_from_str(wants).context("what sluice writes is no YAML")?;
        anyhow::ensure!(docs.len() == 1, "what sluice writes is not one document");
        let mut doc = docs.remove(0);
        for key in &keys {
            walk(&mut doc, &(layout.list)(key))
                .and_then(|l| l.as_vec().map(|_| ()))
                .with_context(|| format!("what sluice writes has no list for {key}'s room"))?;
        }
        if let Some(own) = &layout.own_line {
            walk(&mut doc, &own.at)
                .and_then(|m| m.as_hash().map(|_| ()))
                .context("what sluice writes has no mapping for its further line")?;
        }
        Ok(Wants { doc, keys })
    }

    /// Whether `text` parses as what sluice writes, each room's entries and
    /// the further line taken out.
    fn read_by(&self, text: &str, layout: &Layout<'_>) -> bool {
        let Ok(mut docs) = YamlLoader::load_from_str(text) else {
            return false;
        };
        if docs.len() != 1 {
            return false;
        }
        let mut doc = docs.remove(0);
        let mut wants = self.doc.clone();
        for key in &self.keys {
            let path = (layout.list)(key);
            let Some(Yaml::Array(s)) = walk(&mut wants, &path).cloned() else {
                return false;
            };
            let Some(Yaml::Array(f)) = walk(&mut doc, &path) else {
                return false;
            };
            if !splice(f, &s) {
                return false;
            }
        }
        if let Some(own) = &layout.own_line {
            let Ok(line) = YamlLoader::load_from_str(own.line) else {
                return false;
            };
            let Some(Yaml::Hash(s)) = walk(&mut wants, &own.at).cloned() else {
                return false;
            };
            let Some(Yaml::Hash(f)) = walk(&mut doc, &own.at) else {
                return false;
            };
            for (k, v) in line.first().and_then(Yaml::as_hash).into_iter().flatten() {
                if !s.contains_key(k) && f.get(k) == Some(v) {
                    f.remove(k);
                }
            }
        }
        doc == wants
    }
}

/// `had` read against `wants`, what sluice writes, the rooms and the
/// further line `layout` names: as sluice writes it, with or without the
/// repository's own lines; differing, naming the first line it differs
/// at; or the repository's own lines leaving their place, naming the first
/// line of a room that reaches beyond it.
pub fn read(had: &str, wants: &str, layout: &Layout<'_>) -> Result<Reading> {
    let h = Split::of(had, layout);
    let w = Split::of(wants, layout);
    let ends = h.lines.len();
    if let Some(d) = first_difference(
        &h.numbered(|t| t == Tag::Kept),
        &w.numbered(|t| t == Tag::Kept),
        ends,
    ) {
        return Ok(Reading::Differs(d));
    }
    let keys: Vec<String> = w.rooms.iter().map(|(k, _, _)| k.clone()).collect();
    let parsed = Wants::of(wants, layout, keys)?;
    // The further line alone: where the parse refuses it, it stands where
    // sluice reads none.
    let outside = |t: Tag| !matches!(t, Tag::Room(_));
    if !parsed.read_by(&h.assemble(|_, t| outside(t)), layout) {
        if let Some(d) =
            first_difference(&h.numbered(outside), &w.numbered(|t| t == Tag::Kept), ends)
        {
            return Ok(Reading::Differs(d));
        }
    }
    // Each room in turn, beside those before it: where it reaches beyond
    // its list, the line that does is the one after the most of its lines
    // the parse still reads as sluice writes them.
    for (r, (key, _, lines)) in h.rooms.iter().enumerate() {
        let with = |n: usize| {
            h.assemble(|i, t| match t {
                Tag::Room(q) if q < r => true,
                Tag::Room(q) if q == r => lines.iter().position(|&at| at == i) < Some(n),
                Tag::Room(_) => false,
                _ => true,
            })
        };
        if parsed.read_by(&with(lines.len()), layout) {
            continue;
        }
        let n = (0..lines.len())
            .rev()
            .find(|&n| parsed.read_by(&with(n), layout))
            .unwrap_or(0);
        let at = lines[n];
        return Ok(Reading::Leaves(Leaves {
            key: key.clone(),
            line: at + 1,
            text: h.lines[at].trim_end().to_string(),
        }));
    }
    let mut rooms: Vec<String> = h
        .rooms
        .iter()
        .filter(|(_, _, lines)| lines.iter().any(|&i| !h.lines[i].trim().is_empty()))
        .map(|(k, _, _)| k.clone())
        .collect();
    rooms.dedup();
    Ok(Reading::AsWritten {
        rooms,
        own_line: h.tags.contains(&Tag::Own),
    })
}

/// `wants`, what sluice writes, with each room of `had` carried into its
/// room of the same key, and the further line where `had` holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Carry {
    /// The file written so, and each key of `had`'s rooms holding lines
    /// that `wants` has no room for, which are not carried.
    Carried { text: String, dropped: Vec<String> },
    /// A room of `had` that would reach beyond its place, by its line in
    /// `had`.
    Leaves(Leaves),
}

/// `had`'s rooms carried into `wants` by their keys (design D3).
pub fn carry(had: &str, wants: &str, layout: &Layout<'_>) -> Result<Carry> {
    let h = Split::of(had, layout);
    let w = Split::of(wants, layout);
    let mut by_key: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (key, _, lines) in &h.rooms {
        by_key.entry(key).or_default().extend(lines);
    }
    let own = h.tags.iter().position(|t| *t == Tag::Own);
    let w_plain: Vec<&str> = w
        .lines
        .iter()
        .map(|l| l.trim_end_matches(['\n', '\r']))
        .collect();
    let mut text = String::new();
    // For each line written, the line of `had` it carries, where it does.
    let mut from: Vec<Option<usize>> = Vec::new();
    let opens: BTreeMap<usize, &str> = w
        .rooms
        .iter()
        .map(|(k, open, _)| (*open, k.as_str()))
        .collect();
    let mut carried = std::collections::BTreeSet::new();
    for (i, line) in w.lines.iter().enumerate() {
        text.push_str(line);
        from.push(None);
        if let Some(key) = opens.get(&i) {
            if carried.insert(*key) {
                for &at in by_key.get(key).into_iter().flatten() {
                    text.push_str(h.lines[at]);
                    if !h.lines[at].ends_with('\n') {
                        text.push('\n');
                    }
                    from.push(Some(at));
                }
            }
        }
        if let (Some(at), Some(o)) = (own, &layout.own_line) {
            let next_is_own = w_plain.get(i + 1).is_some_and(|l| l.trim() == o.line);
            if w_plain[i].trim() == o.after && (layout.key)(&w_plain, i) == o.key && !next_is_own {
                let indent = &w_plain[i][..w_plain[i].len() - w_plain[i].trim_start().len()];
                text.push_str(&format!("{indent}{}\n", o.line));
                from.push(Some(at));
            }
        }
    }
    match read(&text, wants, layout)? {
        Reading::AsWritten { .. } => {}
        Reading::Leaves(l) => {
            let at = from
                .get(l.line - 1)
                .copied()
                .flatten()
                .unwrap_or(l.line - 1);
            return Ok(Carry::Leaves(Leaves {
                line: at + 1,
                text: h.lines[at].trim_end().to_string(),
                ..l
            }));
        }
        Reading::Differs(d) => {
            let at = from.get(d.line - 1).copied().flatten();
            anyhow::bail!(
                "the repository's own lines carried into what sluice writes differ from it: {d}{}",
                at.map(|a| format!(" (line {} of the file)", a + 1))
                    .unwrap_or_default()
            )
        }
    }
    let dropped: Vec<String> = by_key
        .iter()
        .filter(|(k, lines)| {
            !carried.contains(*k) && lines.iter().any(|&i| !h.lines[i].trim().is_empty())
        })
        .map(|(k, _)| (*k).to_string())
        .collect();
    Ok(Carry::Carried { text, dropped })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a writer writes: two parts, each with a room among its
    /// entries, and the first part's settings.
    const WRITTEN: &str = "\
# written by a writer
name: example
parts:
  first:
    settings:
      read: yes
    entries:
      - do: one
      # room: yours
      # room: end
      - do: two
  second:
    settings:
      read: yes
    entries:
      - do: three
      # room: yours
      # room: end
";

    /// The part a line at `at` is in: the nearest part's line above it.
    fn part(lines: &[&str], at: usize) -> String {
        lines[..at]
            .iter()
            .rev()
            .find_map(|l| {
                let name = l.strip_prefix("  ")?.strip_suffix(':')?;
                (!name.starts_with(' ')).then(|| name.to_string())
            })
            .unwrap_or_default()
    }

    fn entries(key: &str) -> Vec<Seg> {
        keys(&["parts", key, "entries"])
    }

    fn layout() -> Layout<'static> {
        Layout {
            open: "# room: yours",
            close: "# room: end",
            key: &part,
            list: &entries,
            own_line: Some(OwnLine {
                line: "write: yes",
                after: "read: yes",
                key: "first",
                at: keys(&["parts", "first", "settings"]),
            }),
        }
    }

    /// `WRITTEN` with `lines` put after the line numbered `after`.
    fn with(after: usize, lines: &str) -> String {
        let mut out: Vec<String> = WRITTEN.lines().map(String::from).collect();
        out.insert(after, lines.trim_end_matches('\n').to_string());
        out.join("\n") + "\n"
    }

    /// Design D3: a file as sluice writes it reads so, with or without
    /// lines in its rooms or the further line; one missing a line of a
    /// pair, holding one twice, or differing elsewhere differs, naming the
    /// line; and `carry` writes what sluice writes with each room's lines
    /// in the room of its key, the further line after its anchor, and a
    /// room of a key sluice no longer writes said.
    #[test]
    fn a_room_is_read_emptied_and_carried() {
        let l = layout();
        assert_eq!(
            read(WRITTEN, WRITTEN, &l).unwrap(),
            Reading::AsWritten {
                rooms: vec![],
                own_line: false
            }
        );
        let own = with(
            9,
            "      - do: mine\n        with: a value\n      - do: more",
        );
        let own = own.replacen(
            "      read: yes\n",
            "      read: yes\n      write: yes\n",
            1,
        );
        let reading = read(&own, WRITTEN, &l).unwrap();
        assert_eq!(
            reading,
            Reading::AsWritten {
                rooms: vec!["first".into()],
                own_line: true
            }
        );
        assert!(reading.holds_own());

        // The further line in the second part's settings is no line of the
        // repository's there.
        let second = WRITTEN.replacen(
            "  second:\n    settings:\n      read: yes\n",
            "  second:\n    settings:\n      read: yes\n      write: yes\n",
            1,
        );
        match read(&second, WRITTEN, &l).unwrap() {
            Reading::Differs(d) => {
                assert_eq!(d.line, 15, "{d}");
                assert_eq!(d.had.as_deref(), Some("      write: yes"));
            }
            other => panic!("{other:?}"),
        }
        // Another line where the further line goes differs.
        let other = WRITTEN.replacen(
            "      read: yes\n",
            "      read: yes\n      other: yes\n",
            1,
        );
        assert!(matches!(read(&other, WRITTEN, &l).unwrap(), Reading::Differs(d) if d.line == 7));

        // A pair missing its closing line, or opened twice, differs.
        let unclosed = WRITTEN.replacen("      # room: end\n", "", 1);
        match read(&unclosed, WRITTEN, &l).unwrap() {
            Reading::Differs(d) => {
                assert_eq!(d.line, 10, "{d}");
                assert_eq!(d.wants.as_deref(), Some("      # room: end"));
            }
            other => panic!("{other:?}"),
        }
        let twice = with(9, "      # room: yours");
        assert!(matches!(read(&twice, WRITTEN, &l).unwrap(), Reading::Differs(d) if d.line == 10));
        let elsewhere = WRITTEN.replace("do: two", "do: twice");
        assert_eq!(
            read(&elsewhere, WRITTEN, &l).unwrap(),
            Reading::Differs(Differs {
                line: 11,
                had: Some("      - do: twice".into()),
                wants: Some("      - do: two".into()),
            })
        );

        // Carried into a file sluice writes again: the first part's room
        // and the further line kept; a part it no longer writes said.
        let wants = WRITTEN.replace("# written by a writer", "# written by a later writer");
        let older = format!(
            "{own}  third:\n    entries:\n      # room: yours\n      - do: gone\n      # room: end\n"
        );
        match carry(&older, &wants, &l).unwrap() {
            Carry::Carried { text, dropped } => {
                assert_eq!(dropped, ["third"]);
                assert!(text.starts_with("# written by a later writer\n"));
                assert!(text.contains(
                    "      # room: yours\n      - do: mine\n        with: a value\n      - do: more\n      # room: end\n      - do: two\n"
                ), "{text}");
                assert!(text.contains("      read: yes\n      write: yes\n    entries:"));
                assert_eq!(text.matches("write: yes").count(), 1);
                assert_eq!(
                    read(&text, &wants, &l).unwrap(),
                    Reading::AsWritten {
                        rooms: vec!["first".into()],
                        own_line: true
                    }
                );
            }
            other => panic!("{other:?}"),
        }
        // Carrying a file with nothing of its own writes what sluice writes.
        assert_eq!(
            carry(WRITTEN, &wants, &l).unwrap(),
            Carry::Carried {
                text: wants.clone(),
                dropped: vec![]
            }
        );
    }

    /// Design D3; rule core.62: a room's line less indented than its pair,
    /// which makes a key of its part, and a quoted string opened in a room
    /// and closed in a line of sluice's, each leave the room's place, named
    /// by their line, whether read or carried; a string opened and closed
    /// within the room stays.
    #[test]
    fn a_line_that_leaves_the_room_is_named() {
        let l = layout();
        let less = with(9, "      - do: mine\n    environment: theirs");
        assert_eq!(
            read(&less, WRITTEN, &l).unwrap(),
            Reading::Leaves(Leaves {
                key: "first".into(),
                line: 11,
                text: "    environment: theirs".into(),
            })
        );
        let wants = WRITTEN.replace("# written by a writer", "# written by a later writer");
        assert_eq!(
            carry(&less, &wants, &l).unwrap(),
            Carry::Leaves(Leaves {
                key: "first".into(),
                line: 11,
                text: "    environment: theirs".into(),
            })
        );

        // The string opened in the room swallows sluice's lines up to the
        // quote it closes at.
        let swallowing = WRITTEN.replacen("      - do: two\n", "      - do: \"two\"\n", 1);
        let swallowing = {
            let mut lines: Vec<&str> = swallowing.lines().collect();
            lines.insert(9, "      - do: mine");
            lines.insert(10, "      - do: \"open");
            lines.join("\n") + "\n"
        };
        let w = WRITTEN.replacen("      - do: two\n", "      - do: \"two\"\n", 1);
        assert_eq!(
            read(&swallowing, &w, &l).unwrap(),
            Reading::Leaves(Leaves {
                key: "first".into(),
                line: 11,
                text: "      - do: \"open".into(),
            })
        );

        // A string that opens and closes in the room is the room's own.
        let held = with(
            9,
            "      - do: \"a long\n          line\"\n      - do: after",
        );
        assert!(matches!(
            read(&held, WRITTEN, &l).unwrap(),
            Reading::AsWritten { rooms, .. } if rooms == ["first"]
        ));
        // In the second room, beside a first that stays.
        let second = with(17, "      - do: fine\n  third: added");
        let second = {
            let mut lines: Vec<&str> = second.lines().collect();
            lines.insert(9, "      - do: mine");
            lines.join("\n") + "\n"
        };
        assert_eq!(
            read(&second, WRITTEN, &l).unwrap(),
            Reading::Leaves(Leaves {
                key: "second".into(),
                line: 20,
                text: "  third: added".into(),
            })
        );
    }
}
