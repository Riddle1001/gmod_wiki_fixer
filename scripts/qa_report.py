"""Run report interaction, mobile, and no-JS checks against any generated report.
Requires Python Playwright and Chromium. Fixtures and real reports are both supported.
"""
import argparse
import json
from pathlib import Path
from playwright.sync_api import sync_playwright

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('report', nargs='?', type=Path, default=root / 'reports/wiki-audit.html')
parser.add_argument('--output', type=Path, default=root / '.cache/report-qa')
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
with sync_playwright() as p:
    browser = p.chromium.launch(headless=True)
    page = browser.new_page(viewport={'width':1280,'height':900})
    errors = []
    page.on('pageerror', lambda error: errors.append(str(error)))
    page.goto(args.report.resolve().as_uri())
    page.wait_for_function("document.documentElement.classList.contains('js')")
    # Local-file fetch is unavailable in some browsers; the shared check uses
    # the same document through HTTP for its no-JS frame. Serve reports locally
    # when using the shared expression, as below.
    import functools
    import threading
    from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
    handler = functools.partial(SimpleHTTPRequestHandler, directory=str(args.report.resolve().parent))
    server = ThreadingHTTPServer(('127.0.0.1',0), handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        from urllib.parse import quote
        page.goto(f'http://127.0.0.1:{server.server_port}/{quote(args.report.name)}')
        result = page.evaluate((root / 'scripts/report_browser_checks.js').read_text(encoding='utf-8-sig'))
        # Printing must expose the complete record even when a focused view is active.
        page.emulate_media(media='print')
        print_check = page.evaluate("""() => ({
            recordVisible: getComputedStyle(document.getElementById('source-data')).display !== 'none',
            qualityVisible: getComputedStyle(document.getElementById('quality-data')).display !== 'none',
            coverageVisible: getComputedStyle(document.getElementById('coverage')).display !== 'none',
            queueHidden: getComputedStyle(document.getElementById('review')).display === 'none'
        })""")
        result['results'].append({'name':'print retains complete report', 'passed':all(print_check.values()), 'detail':print_check})
        result['passed'] = result['passed'] and all(print_check.values())
        page.emulate_media(media='screen')
        page.screenshot(path=str(args.output / 'desktop.png'), full_page=True)
        page.set_viewport_size({'width':390,'height':844})
        page.screenshot(path=str(args.output / 'mobile.png'), full_page=True)
        page.goto(f'http://127.0.0.1:{server.server_port}/{quote(args.report.name)}?quality=all#documentation-quality')
        page.wait_for_function("document.documentElement.classList.contains('js')")
        page.screenshot(path=str(args.output / 'quality-mobile.png'), full_page=True)
        page.set_viewport_size({'width':1280,'height':900})
        page.screenshot(path=str(args.output / 'quality-desktop.png'), full_page=True)
        result['browser_errors'] = errors
        (args.output / 'checks.json').write_text(json.dumps(result, indent=2), encoding='utf-8')
        print(json.dumps({'passed':result['passed'] and not errors,'log':str(args.output / 'checks.json')}))
    finally:
        server.shutdown(); server.server_close(); thread.join(); browser.close()
raise SystemExit(not result['passed'] or bool(errors))
