#!/usr/bin/env python3
"""Burner launcher.

Serves the browser app from this folder on this computer only (127.0.0.1) and opens it
in your web browser. Browsers allow USB (Web Serial) connections to a laser only from
pages served on localhost or https, so use this instead of opening index.html directly.

    python3 main.py                 start and open the browser
    python3 main.py --port 8765     choose the port
    python3 main.py --no-browser    only serve
    python3 main.py --screenshot out.png   render the app headless, save a picture, exit
"""
import argparse
import functools
import http.server
import os
import shutil
import socket
import socketserver
import subprocess
import sys
import tempfile
import threading
import webbrowser

HERE = os.path.dirname(os.path.abspath(__file__))


class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map,
                      '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css',
                      '.svg': 'image/svg+xml', '.json': 'application/json'}

    def end_headers(self):
        self.send_header('Cache-Control', 'no-store')
        super().end_headers()

    def log_message(self, fmt, *args):
        pass


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]


def serve(port):
    handler = functools.partial(Handler, directory=HERE)
    httpd = Server(('127.0.0.1', port), handler)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    return httpd


def find_browser():
    names = ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser', 'microsoft-edge', 'msedge', 'chrome']
    for n in names:
        p = shutil.which(n)
        if p:
            return p
    for p in [r'C:\Program Files\Google\Chrome\Application\chrome.exe',
              r'C:\Program Files (x86)\Google\Chrome\Application\chrome.exe',
              r'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe',
              r'C:\Program Files\Microsoft\Edge\Application\msedge.exe',
              '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
              '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
              '/Applications/Chromium.app/Contents/MacOS/Chromium']:
        if os.path.exists(p):
            return p
    return None


def screenshot(out):
    browser = find_browser()
    if not browser:
        print('error: no Chrome, Chromium or Edge found to render the app', file=sys.stderr)
        return 1
    port = free_port()
    httpd = serve(port)
    profile = tempfile.mkdtemp(prefix='burner-shot-')
    out = os.path.abspath(out)
    os.makedirs(os.path.dirname(out) or '.', exist_ok=True)
    if os.path.exists(out):
        os.remove(out)
    try:
        # ~120 frames at 60 fps = 2 s of page time, plus start-up
        cmd = [browser, '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check',
               '--hide-scrollbars', f'--user-data-dir={profile}', '--window-size=1440,900',
               '--virtual-time-budget=3000', f'--screenshot={out}', f'http://127.0.0.1:{port}/index.html?shot=1']
        if hasattr(os, 'geteuid') and os.geteuid() == 0:
            cmd.insert(1, '--no-sandbox')
        subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120)
    except subprocess.TimeoutExpired:
        print('error: the browser took too long to render the app', file=sys.stderr)
        return 1
    finally:
        httpd.shutdown()
        shutil.rmtree(profile, ignore_errors=True)
    if not os.path.exists(out) or os.path.getsize(out) == 0:
        print('error: the browser did not save a picture', file=sys.stderr)
        return 1
    print(f'saved {out}')
    return 0


def main():
    ap = argparse.ArgumentParser(description='Burner: design and send laser jobs from your browser.')
    ap.add_argument('--port', type=int, default=8765, help='port to serve on (default 8765)')
    ap.add_argument('--no-browser', action='store_true', help="don't open a browser window")
    ap.add_argument('--screenshot', metavar='FILE.png', help='render the app, save a picture and exit')
    a = ap.parse_args()
    if a.screenshot:
        return screenshot(a.screenshot)
    try:
        httpd = serve(a.port)
    except OSError:
        a.port = free_port()
        httpd = serve(a.port)
    url = f'http://localhost:{a.port}/'
    print(f'Burner is running at {url}')
    print('Use Chrome or Edge to connect to a laser over USB. Press Ctrl+C to quit.')
    if not a.no_browser:
        webbrowser.open(url)
    try:
        threading.Event().wait()
    except KeyboardInterrupt:
        print('\nStopped.')
    httpd.shutdown()
    return 0


if __name__ == '__main__':
    sys.exit(main())
