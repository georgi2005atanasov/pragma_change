"""Example verification-aware backend. Replace with your application."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        data = json.dumps({"path": self.path, "body": body.decode("utf-8", errors="replace"),
                           "risk": {k.lower(): v for k, v in self.headers.items() if k.lower().startswith("x-pragmachange-")},
                           "verification_required": self.headers.get("X-PragmaChange-Action") == "check"}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
    do_GET = do_POST

if __name__ == "__main__":
    ThreadingHTTPServer((os.environ.get("HOST", "0.0.0.0"), int(os.environ.get("PORT", "9000"))), Handler).serve_forever()
