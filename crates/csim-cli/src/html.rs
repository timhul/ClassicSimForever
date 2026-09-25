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
h2 {
  color: var(--yellow);
  font-size: 1.15rem;
  margin: 36px 0 10px;
  padding-left: 10px;
  border-left: 4px solid var(--yellow);
}
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
tbody tr:nth-child(even) { background: var(--panel); }
tbody tr:hover { background: #221d00; }
td:first-child { color: #fff; }
"#;

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
         <p class=\"meta\">{} iterations of {} s · {} threads · seed {} · {:.2} s ({} events)</p>\n\
         </header>\n",
        escape(&setup.name),
        escape(&setup.race),
        escape(&setup.class),
        escape(&setup.rotation),
        escape(&setup.phase),
        escape(&setup.ruleset),
        run.iterations,
        run.combat_length,
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
    for (name, value) in [
        ("TPS", results.tps),
        ("Std dev", dps.standard_deviation),
        ("Min", dps.min),
        ("Max", dps.max),
    ] {
        let _ = writeln!(
            out,
            "<div class=\"stat\"><div class=\"name\">{name}</div>\
             <div class=\"value\">{value:.2}</div></div>"
        );
    }
    out.push_str("</div>\n</section>\n");

    for (title, table) in results.tables() {
        let _ = writeln!(out, "<h2>{}</h2>", escape(title));
        table_html(&mut out, &table);
    }
    out.push_str("</main>\n</body>\n</html>\n");
    out
}

fn table_html(out: &mut String, table: &Table) {
    let align = |column| {
        if table.is_left(column) {
            " class=\"left\""
        } else {
            ""
        }
    };
    out.push_str("<div class=\"scroll\"><table>\n<thead><tr>");
    for (column, header) in table.headers().iter().enumerate() {
        let _ = write!(out, "<th{}>{}</th>", align(column), escape(header));
    }
    out.push_str("</tr></thead>\n<tbody>\n");
    for row in table.rows() {
        out.push_str("<tr>");
        for (column, cell) in row.iter().enumerate() {
            let _ = write!(out, "<td{}>{}</td>", align(column), escape(cell));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</tbody>\n</table></div>\n");
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
        table_html(&mut out, &table);
        assert!(
            out.contains("<th class=\"left\">Buff</th><th class=\"left\">Kind</th><th>Uptime</th>")
        );
        assert!(out.contains("<td class=\"left\">Flurry</td>"));
        assert!(out.contains("<td>50.0%</td>"));
    }
}
