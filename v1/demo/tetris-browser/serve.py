#!/usr/bin/env python3
"""HTTP server with COOP/COEP headers for SharedArrayBuffer (required by wllama Memory64).

Serves from the llm_os repo root so the demo's `../../kernel/...` and
`../../cart/...` imports resolve. Open http://localhost:PORT/demo/tetris-browser/
"""
import http.server
import os
import sys

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8888
REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))

class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=REPO_ROOT, **kwargs)
    def end_headers(self):
        self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'credentialless')
        super().end_headers()

print(f'Serving at http://localhost:{PORT}/demo/tetris-browser/ (with COOP/COEP headers)')
http.server.HTTPServer(('', PORT), Handler).serve_forever()
