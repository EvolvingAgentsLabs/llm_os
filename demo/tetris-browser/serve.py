#!/usr/bin/env python3
"""HTTP server with COOP/COEP headers for SharedArrayBuffer (required by wllama Memory64)."""
import http.server
import sys

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8888

class Handler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'credentialless')
        super().end_headers()

print(f'Serving at http://localhost:{PORT} (with COOP/COEP headers)')
http.server.HTTPServer(('', PORT), Handler).serve_forever()
