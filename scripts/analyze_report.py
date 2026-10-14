"""Summarize source discrepancies from the current HTML report (standard library only)."""
import argparse
from html.parser import HTMLParser
from pathlib import Path


class Findings(HTMLParser):
    def __init__(self):
        super().__init__()
        self.rows = []
        self.row = None
        self.cell = None

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "tr" and attrs.get("data-category") in {
            "line-ranges", "wrong-files", "missing-source", "dead-files",
            "realm-mismatches", "missing-realms",
        }:
            self.row = []
        if tag == "td" and self.row is not None:
            self.cell = []

    def handle_data(self, data):
        if self.cell is not None:
            self.cell.append(data)

    def handle_endtag(self, tag):
        if tag == "td" and self.cell is not None:
            self.row.append(" ".join(self.cell).strip())
            self.cell = None
        if tag == "tr" and self.row is not None:
            if len(self.row) == 5:
                self.rows.append(self.row)
            self.row = None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", nargs="?", type=Path, default=Path("audit_report.html"))
    parser.add_argument("--limit", type=int, default=15)
    args = parser.parse_args()
    report = Findings()
    html = args.report.read_text(encoding="utf-8")
    report.feed(html)
    label = "Page views" if "const pageViews =" in html else "Score"
    def priority(row):
        return int(row[0]) if row[0].isdigit() else -1
    for row in sorted(report.rows, key=priority, reverse=True)[:args.limit]:
        print(f"{label} {row[0]} | {row[1]}\n  Wiki: {row[2]}\n  Source: {row[3]}\n  {row[4]}")


if __name__ == "__main__":
    main()
