//! `--output-format html`: the results as one self-contained page in black and yellow, the DPS
//! as the headline.

use std::fmt::Write;

use crate::run::Results;
use crate::table::Table;

const STYLE: &str = r#"
:root {
  --bg: #0a0a0a;
  --panel: #151515;
  --line: #2a2a2a;
  --text: #e8e8e8;
  --muted: #9a9a9a;
  --yellow: #ffd100;
  --yellow-dim: #b89600;
}
* { box-sizing: border-box; }
body {
  margin: 0;
  background: var(--bg);
  color: var(--text);
  font: 15px/1.5 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
}
main { max-width: 1100px; margin: 0 auto; padding: 32px 16px 64px; }
header { border-bottom: 2px solid var(--yellow); padding-bottom: 16px; }
h1 { margin: 0; color: var(--yellow); font-size: 1.9rem; letter-spacing: 0.01em; }
.subtitle, .meta { margin: 4px 0 0; color: var(--muted); }
.hero {
  margin: 28px 0;
  padding: 28px 24px;
  background: var(--panel);
  border: 1px solid var(--line);
  border-left: 6px solid var(--yellow);
  border-radius: 6px;
}
.hero .label {
  color: var(--yellow-dim);
  font-weight: 700;
  letter-spacing: 0.2em;
  text-transform: uppercase;
  font-size: 0.85rem;
}
.dps {
  color: var(--yellow);
  font-size: clamp(3.5rem, 12vw, 6.5rem);
  font-weight: 800;
  line-height: 1;
  margin: 6px 0 4px;
  font-variant-numeric: tabular-nums;
  text-shadow: 0 0 24px rgba(255, 209, 0, 0.25);
}
.ci { color: var(--text); font-size: 1.15rem; }
.stats { display: flex; flex-wrap: wrap; gap: 12px 32px; margin-top: 20px; }
.stat .name { color: var(--muted); font-size: 0.8rem; text-transform: uppercase; letter-spacing: 0.1em; }
.stat .value { font-size: 1.25rem; font-weight: 600; font-variant-numeric: tabular-nums; }
details { margin-top: 36px; }
summary {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 10px;
  padding-left: 10px;
  border-left: 4px solid var(--yellow);
  cursor: pointer;
  list-style: none;
  user-select: none;
}
summary::-webkit-details-marker { display: none; }
summary::after {
  content: "\25B8";
  color: var(--yellow-dim);
  transition: transform 0.15s;
}
details[open] > summary::after { transform: rotate(90deg); }
summary:hover h2, summary:hover::after { color: #fff; }
summary:focus-visible { outline: 2px solid var(--yellow); outline-offset: 4px; }
h2 { color: var(--yellow); font-size: 1.15rem; margin: 0; }
.scroll { overflow-x: auto; border: 1px solid var(--line); border-radius: 6px; }
table { border-collapse: collapse; width: 100%; font-variant-numeric: tabular-nums; }
th, td { padding: 6px 12px; white-space: nowrap; text-align: right; }
th.left, td.left { text-align: left; }
th {
  background: var(--yellow);
  color: #000;
  font-weight: 700;
  position: sticky;
  top: 0;
}
tbody tr { border-top: 1px solid var(--line); }
tbody tr:nth-child(even of :not(.sub)) { background: var(--panel); }
tr.sub { background: #0f0f0f; color: var(--muted); }
tr.sub td:first-child { color: var(--muted); padding-left: 32px; }
button.toggle {
  all: unset;
  cursor: pointer;
}
button.toggle::before {
  content: "\25B8";
  display: inline-block;
  width: 1em;
  color: var(--yellow-dim);
  transition: transform 0.15s;
}
button.toggle[aria-expanded="true"]::before { transform: rotate(90deg); }
button.toggle:hover, button.toggle:hover::before { color: var(--yellow); }
button.toggle:focus-visible { outline: 2px solid var(--yellow); outline-offset: 2px; }
tbody tr:hover { background: #221d00; }
td:first-child { color: #fff; }
tfoot tr { border-top: 2px solid var(--yellow); }
tfoot td, tfoot td:first-child { color: var(--yellow); font-weight: 700; }
th { cursor: pointer; user-select: none; }
th:hover { background: #ffe066; }
th[aria-sort="ascending"]::after { content: " \25B4"; }
th[aria-sort="descending"]::after { content: " \25BE"; }
"#;

/// Sorts a table's body rows on the clicked header: numbers largest first and text A to Z, a second
/// click reverses. Empty cells stay last, the totals in `tfoot` stay put, and breakdown rows
/// (`tr.sub`) move with the row above them. A row's toggle shows or hides its breakdown.
const SCRIPT: &str = r#"
document.querySelectorAll("button.toggle").forEach((button) => button.addEventListener("click", () => {
  const open = button.getAttribute("aria-expanded") !== "true";
  button.setAttribute("aria-expanded", open);
  for (let row = button.closest("tr").nextElementSibling; row && row.classList.contains("sub"); row = row.nextElementSibling) {
    row.hidden = !open;
  }
}));
document.querySelectorAll("th").forEach((th) => th.addEventListener("click", () => {
  const table = th.closest("table"), body = table.tBodies[0], column = th.cellIndex;
  if (!body) return;
  const text = th.classList.contains("left");
  const descending = th.getAttribute("aria-sort") ? th.getAttribute("aria-sort") === "ascending" : !text;
  table.querySelectorAll("th").forEach((other) => other.removeAttribute("aria-sort"));
  th.setAttribute("aria-sort", descending ? "descending" : "ascending");
  const key = (row) => {
    const cell = row.cells[column].textContent.trim();
    if (text) return cell === "" ? null : cell;
    const number = parseFloat(cell.replace(/,/g, ""));
    return Number.isNaN(number) ? null : number;
  };
  const groups = [];
  for (const row of body.rows) {
    if (row.classList.contains("sub") && groups.length) groups[groups.length - 1].push(row);
    else groups.push([row]);
  }
  groups.sort(([a], [b]) => {
    const x = key(a), y = key(b);
    if (x === null || y === null) return (x === null) - (y === null);
    const order = text ? x.localeCompare(y) : x - y;
    return descending ? -order : order;
  });
  body.append(...groups.flat());
}));
"#;

/// Sections that start collapsed.
const COLLAPSED: [&str; 3] = ["Rotation", "Skipped rotation lines", "Engine"];

/// Sections that start sorted on a column, largest first.
const SORTED: [(&str, &str); 1] = [("Stat weights", "DPS")];

/// The results as an HTML page.
pub fn render(results: &Results) -> String {
    let (setup, run, dps) = (&results.setup, &results.run, &results.dps);
    let mut out = String::new();
    let _ = write!(
        out,
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{} – {:.2} DPS</title>\n<style>{STYLE}</style>\n</head>\n<body>\n<main>\n",
        escape(&setup.name),
        dps.mean,
    );

    let _ = write!(
        out,
        "<header>\n<h1>{}</h1>\n<p class=\"subtitle\">{} {} · rotation {} · {} · {} ruleset</p>\n\
         <p class=\"meta\">{} iterations of {} s ± {}% · {} threads · seed {} · {:.2} s ({} events)</p>\n\
         </header>\n",
        escape(&setup.name),
        escape(&setup.race),
        escape(&setup.class),
        escape(&setup.rotation),
        escape(&setup.phase),
        escape(&setup.ruleset),
        run.iterations,
        run.combat_length,
        run.length_variance,
        run.threads,
        run.seed,
        run.elapsed_seconds,
        run.events,
    );

    let _ = write!(
        out,
        "<section class=\"hero\">\n<div class=\"label\">Damage per second</div>\n\
         <div class=\"dps\">{:.2}</div>\n<div class=\"ci\">± {:.2} (95% CI)</div>\n\
         <div class=\"stats\">\n",
        dps.mean, dps.confidence_interval,
    );
    let raid = results
        .raid
        .iter()
        .flat_map(|raid| [("Raid DPS", raid.dps), ("Raid TPS", raid.tps)]);
    for (name, value) in [
        ("TPS", results.tps),
        ("Std dev", dps.standard_deviation),
        ("Min", dps.min),
        ("Max", dps.max),
    ]
    .into_iter()
    .chain(raid)
    {
        let _ = writeln!(
            out,
            "<div class=\"stat\"><div class=\"name\">{name}</div>\
             <div class=\"value\">{value:.2}</div></div>"
        );
    }
    out.push_str("</div>\n</section>\n");

    for (title, table) in results.tables() {
        let open = if COLLAPSED.contains(&title) {
            ""
        } else {
            " open"
        };
        let _ = writeln!(
            out,
            "<details{open}>\n<summary><h2>{}</h2></summary>",
            escape(title)
        );
        let sorted = SORTED
            .iter()
            .find(|(section, _)| *section == title)
            .and_then(|(_, header)| table.headers().iter().position(|h| h == header));
        table_html(&mut out, &table, sorted);
        out.push_str("</details>\n");
    }
    let _ = write!(
        out,
        "</main>\n<script>{SCRIPT}</script>\n</body>\n</html>\n"
    );
    out
}

/// Writes `table`, its body rows sorted on the numeric column `sorted` largest first (like a click
/// on its header would) when given.
fn table_html(out: &mut String, table: &Table, sorted: Option<usize>) {
    let align = |column| {
        if table.is_left(column) {
            " class=\"left\""
        } else {
            ""
        }
    };
    out.push_str("<div class=\"scroll\"><table>\n<thead><tr>");
    for (column, header) in table.headers().iter().enumerate() {
        let sort = if sorted == Some(column) {
            " aria-sort=\"descending\""
        } else {
            ""
        };
        let _ = write!(out, "<th{}{sort}>{}</th>", align(column), escape(header));
    }
    out.push_str("</tr></thead>\n");
    // Body rows with their breakdowns, totals without.
    let mut body: Vec<_> = (0..table.rows().len())
        .map(|row| (&table.rows()[row], table.sub_rows(row)))
        .collect();
    if let Some(column) = sorted {
        let key = |row: &Vec<String>| row[column].replace(',', "").parse::<f64>().ok();
        // Largest first, cells without a number last.
        body.sort_by(|(a, _), (b, _)| match (key(a), key(b)) {
            (Some(x), Some(y)) => y.total_cmp(&x),
            (x, y) => x.is_none().cmp(&y.is_none()),
        });
    }
    let totals: Vec<_> = table.totals().iter().map(|row| (row, &[][..])).collect();
    for (part, rows) in [("tbody", body), ("tfoot", totals)] {
        if rows.is_empty() {
            continue;
        }
        let _ = writeln!(out, "<{part}>");
        for (row, sub_rows) in rows {
            out.push_str("<tr>");
            for (column, cell) in row.iter().enumerate() {
                if column == 0 && !sub_rows.is_empty() {
                    let _ = write!(
                        out,
                        "<td{}><button type=\"button\" class=\"toggle\" \
                         aria-expanded=\"false\">{}</button></td>",
                        align(column),
                        escape(cell)
                    );
                } else {
                    let _ = write!(out, "<td{}>{}</td>", align(column), escape(cell));
                }
            }
            out.push_str("</tr>\n");
            // Collapsed until the row's toggle opens them.
            for sub in sub_rows {
                out.push_str("<tr class=\"sub\" hidden>");
                for (column, cell) in sub.iter().enumerate() {
                    let _ = write!(out, "<td{}>{}</td>", align(column), escape(cell));
                }
                out.push_str("</tr>\n");
            }
        }
        let _ = writeln!(out, "</{part}>");
    }
    out.push_str("</table></div>\n");
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_in_names_is_escaped() {
        assert_eq!(
            escape("<Tom & Jerry's \"Axe\">"),
            "&lt;Tom &amp; Jerry&#39;s &quot;Axe&quot;&gt;"
        );
    }

    #[test]
    fn tables_keep_their_alignment() {
        let mut table = Table::new(["Buff", "Kind", "Uptime"]).left(1);
        table.row(vec!["Flurry".into(), "buff".into(), "50.0%".into()]);
        let mut out = String::new();
        table_html(&mut out, &table, None);
        assert!(
            out.contains("<th class=\"left\">Buff</th><th class=\"left\">Kind</th><th>Uptime</th>")
        );
        assert!(out.contains("<td class=\"left\">Flurry</td>"));
        assert!(out.contains("<td>50.0%</td>"));
    }

    #[test]
    fn sub_rows_start_collapsed_under_their_row() {
        let mut table = Table::new(["Spell", "DPS"]);
        table.row_with_sub_rows(
            vec!["Touch of the Grave".into(), "5.0".into()],
            vec![
                vec!["100% resisted".into(), "".into()],
                vec!["0% resisted".into(), "5.0".into()],
            ],
        );
        table.row(vec!["Bloodthirst".into(), "300.0".into()]);
        let mut out = String::new();
        table_html(&mut out, &table, Some(1));
        assert!(
            out.contains(
                "<tr><td class=\"left\">Bloodthirst</td><td>300.0</td></tr>\n\
                 <tr><td class=\"left\"><button type=\"button\" class=\"toggle\" aria-expanded=\"false\">\
                 Touch of the Grave</button></td><td>5.0</td></tr>\n\
                 <tr class=\"sub\" hidden><td class=\"left\">100% resisted</td><td></td></tr>\n\
                 <tr class=\"sub\" hidden><td class=\"left\">0% resisted</td><td>5.0</td></tr>\n"
            ),
            "{out}"
        );
    }

    #[test]
    fn sorted_tables_start_largest_first() {
        let mut table = Table::new(["Option", "DPS"]);
        for (option, dps) in [
            ("Strength", "+1.50"),
            ("Hit", ""),
            ("Crit", "+20.25"),
            ("Agility", "-0.50"),
        ] {
            table.row(vec![option.into(), dps.into()]);
        }
        let mut out = String::new();
        table_html(&mut out, &table, Some(1));
        assert!(
            out.contains("<th class=\"left\">Option</th><th aria-sort=\"descending\">DPS</th>")
        );
        let order: Vec<_> = ["Crit", "Strength", "Agility", "Hit"]
            .iter()
            .map(|option| {
                out.find(&format!("<td class=\"left\">{option}</td>"))
                    .unwrap()
            })
            .collect();
        assert!(order.is_sorted(), "{out}");
    }
}
