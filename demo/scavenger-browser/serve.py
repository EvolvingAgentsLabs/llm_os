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
import socketserver

class COOPCOEPHandler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        super().end_headers()

if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--port", type=int, default=8889)
    args = p.parse_args()
    with socketserver.TCPServer(("", args.port), COOPCOEPHandler) as httpd:
        print(f"scavenger demo at http://localhost:{args.port}")
        httpd.serve_forever()
