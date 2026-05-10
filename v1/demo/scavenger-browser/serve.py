#!/usr/bin/env python3
"""
Tiny HTTP server with COOP/COEP headers, required by SharedArrayBuffer
(which wllama needs for its multithreaded WASM build).

Usage:
    cd demo/scavenger-browser
    python3 serve.py            # serves on http://localhost:8889
    python3 serve.py --port 9000

Then open the printed URL in a recent Chrome/Firefox.
"""
import argparse
import http.server
import os
import socketserver

# Serve from the llm_os repo root so the demo's `../../kernel/...` and
# `../../cart/...` imports resolve. The demo HTML lives at
# /demo/scavenger-browser/index.html under that root.
REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))

class COOPCOEPHandler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=REPO_ROOT, **kwargs)
    def end_headers(self):
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        super().end_headers()

if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--port", type=int, default=8889)
    args = p.parse_args()
    with socketserver.TCPServer(("", args.port), COOPCOEPHandler) as httpd:
        print(f"scavenger demo at http://localhost:{args.port}/demo/scavenger-browser/")
        httpd.serve_forever()
