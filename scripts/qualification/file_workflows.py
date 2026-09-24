#!/usr/bin/env python3
"""Loopback-only file workflow fixture. Never serves arbitrary local files.

Run with --files /private/tmp/zephium-upload-fixtures. The printed URL and
generated files are sufficient for an actual-product picker/upload test.
Received bodies are bounded, compared against fixed fixture bytes and discarded.
"""

import argparse
import hashlib
import json
import secrets
import time
from email import policy
from email.parser import BytesParser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

FILES = {
    "zephium-upload.txt": b"Zephium native upload fixture\n\x00exact bytes\n",
    "second-α.txt": "Second file: Unicode α\n".encode(),
    "folder/nested/third.txt": b"Nested directory fixture\n",
}
MAX_BODY = 1024 * 1024


def verify_upload(content_type, body):
    message = BytesParser(policy=policy.default).parsebytes(
        b"Content-Type: " + content_type.encode("ascii") + b"\r\nMIME-Version: 1.0\r\n\r\n" + body
    )
    if not message.is_multipart():
        raise ValueError("Expected multipart form data")
    expected = {hashlib.sha256(data).hexdigest(): len(data) for data in FILES.values()}
    received = []
    for part in message.iter_parts():
        if not part.get_filename():
            continue
        data = part.get_payload(decode=True)
        digest = hashlib.sha256(data).hexdigest()
        received.append({"bytes": len(data), "sha256": digest,
                         "matches_fixture": expected.get(digest) == len(data)})
    return {"ok": bool(received) and all(entry["matches_fixture"] for entry in received),
            "files": received}


class FixtureHandler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass  # Do not log request paths, filenames or received contents.

    def send_body(self, status, content_type, body):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        if content_type.startswith("text/html"):
            self.send_header("Set-Cookie", f"fixture_session={self.server.token}; HttpOnly; SameSite=Strict; Path=/{self.server.token}")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        base = "/" + self.server.token
        if self.path == base + "/empty":
            self.send_attachment("zephium-empty.txt", b"")
            return
        if self.path == base + "/truncated":
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Disposition", 'attachment; filename="zephium-truncated.bin"')
            self.send_header("Content-Length", "1048576")
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(b"deliberately incomplete fixture")
            self.wfile.flush()
            self.close_connection = True
            return
        if self.path == base + "/unknown-length":
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Disposition", 'attachment; filename="zephium-unknown-length.txt"')
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(b"Unknown-length fixture\n")
            self.close_connection = True
            return
        if self.path == base + "/authenticated":
            if f"fixture_session={self.server.token}" not in self.headers.get("Cookie", "").split("; "):
                self.send_body(403, "text/plain", b"Fixture cookie missing")
                return
            self.send_attachment("zephium-authenticated.txt", b"Cookie-authenticated fixture\n")
            return
        if self.path in (base + "/download", base + "/slow", base + "/redirect"):
            if self.path.endswith("/redirect"):
                self.send_response(302)
                self.send_header("Location", base + "/download")
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            slow = self.path.endswith("/slow")
            data = b"Zephium native download fixture\n"
            data = (data * (65536 // len(data) + 1))[:65536] if slow else data
            count = 512 if slow else 1
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Disposition", 'attachment; filename="zephium-slow.bin"' if slow else 'attachment; filename="zephium-download.txt"')
            self.send_header("Content-Length", str(len(data) * count))
            self.end_headers()
            try:
                for _ in range(count):
                    self.wfile.write(data)
                    self.wfile.flush()
                    if slow:
                        time.sleep(0.08)
            except (BrokenPipeError, ConnectionResetError):
                pass
            return
        if self.path not in (base, base + "/frame", base + "/away"):
            self.send_body(404, "text/plain", b"Not found")
            return
        if self.path.endswith("/away"):
            self.send_body(200, "text/html", b"<title>Navigated</title><h1>Navigated away</h1>")
            return
        frame = self.path.endswith("/frame")
        nested = "" if frame else f'<iframe title="Cross-origin upload" src="http://localhost:{self.server.server_port}{base}/frame"></iframe>'
        page = r'''<!doctype html><meta charset="utf-8"><title>Zephium file workflow fixture</title>
<style>body{font:16px system-ui;margin:32px;max-width:850px}label{display:block;margin:20px 0}button{padding:8px 16px}pre{white-space:pre-wrap}iframe{width:100%;height:360px}#drop{padding:20px;border:2px dashed #888}</style>
<h1>Native file uploads</h1>
<p>Select only the generated fixture files. Uploads stay on this loopback server.</p>
<p><a href="__BASE__/download">Download attachment</a> · <a href="__BASE__/slow">Slow download (32 MiB)</a> · <a href="__BASE__/redirect">Redirected download</a></p>
<p><button id="blob">Download generated text</button> <button id="data">Download data URL</button></p>
<p><a href="__BASE__/empty">Empty file</a> · <a href="__BASE__/unknown-length">Unknown length</a> · <a href="__BASE__/truncated">Truncated response (must fail)</a></p>
<p><a href="__BASE__/authenticated">Cookie-authenticated text attachment</a></p>
<form method="POST" action="__BASE__/export"><input type="hidden" name="fixture" value="generated"><button>Download POST export</button></form>
<form id="upload">
<label>Single file <input type="file" name="single" id="single"></label>
<label>Multiple files <input type="file" name="multiple" multiple></label>
<label>Text hint <input type="file" name="text" accept=".txt,text/plain"></label>
<label>Directory <input type="file" name="folder" webkitdirectory></label>
<button type="submit">Upload selected files</button>
<button type="button" id="reset">Reset</button></form>
<p><button id="navigate">Navigate in 4 seconds</button> Then open a picker to test cancellation.</p>
<p><button id="untrusted">Try delayed scripted picker</button></p>
<div id="drop">Drop generated fixture files here to upload</div><pre id="result" aria-live="polite">No upload yet</pre>
__FRAME__
<script>
const form = document.querySelector('#upload'), result = document.querySelector('#result');
document.querySelector('#blob').onclick = () => {
  const url = URL.createObjectURL(new Blob(['Zephium generated download fixture\n'], {type:'text/plain'}));
  const link = document.createElement('a'); link.href=url; link.download='zephium-generated.txt'; link.click();
  setTimeout(() => URL.revokeObjectURL(url),60000);
};
document.querySelector('#data').onclick = () => {
  const link = document.createElement('a');
  link.href='data:text/plain;charset=utf-8,'+encodeURIComponent('Zephium data URL fixture\n'.repeat(1024));
  link.download='zephium-data.txt';link.click();
};
async function send(data) {
  try {
    const response = await fetch('__BASE__/upload', {method:'POST',body:data});
    const report = await response.json();
    result.textContent = JSON.stringify(report,null,2);
    document.title = report.ok ? 'PASS: exact fixture bytes received' : 'FAIL: upload mismatch';
  } catch(error) { result.textContent = String(error); }
}
form.addEventListener('submit', event => {event.preventDefault();send(new FormData(form));});
document.querySelector('#reset').onclick = () => {form.reset();result.textContent='No upload yet';};
document.querySelector('#navigate').onclick = () => setTimeout(() => location.href='__BASE__/away',4000);
document.querySelector('#untrusted').onclick = () => setTimeout(() => document.querySelector('#single').click(),6000);
const drop = document.querySelector('#drop');
drop.ondragover = event => event.preventDefault();
drop.ondrop = event => {event.preventDefault();const data=new FormData();for(const file of event.dataTransfer.files)data.append('dropped',file);send(data);};
</script>'''.replace("__BASE__", base).replace("__FRAME__", nested)
        self.send_body(200, "text/html; charset=utf-8", page.encode())

    def send_attachment(self, name, data):
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Disposition", f'attachment; filename="{name}"')
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        base = "/" + self.server.token
        origins = {f"http://{host}:{self.server.server_port}" for host in ("127.0.0.1", "localhost")}
        if self.path == base + "/export" and self.headers.get("Origin") in origins:
            self.connection.settimeout(5)
            if self.headers.get("Content-Length") != "17" or self.rfile.read(17) != b"fixture=generated":
                self.send_body(400, "text/plain", b"Invalid generated export request")
                return
            self.send_attachment("zephium-post-export.txt", b"POST export fixture\n")
            return
        if self.path != base + "/upload" or self.headers.get("Origin") not in origins:
            self.send_body(403, "text/plain", b"Forbidden")
            return
        try:
            count = int(self.headers.get("Content-Length", "0"))
            if not 0 < count <= MAX_BODY:
                raise ValueError("Body must be between 1 byte and 1 MiB")
            self.connection.settimeout(5)
            body = self.rfile.read(count)
            if len(body) != count:
                raise ValueError("Truncated request")
            report = verify_upload(self.headers.get("Content-Type", ""), body)
            print(json.dumps(report), flush=True)
            self.send_body(200, "application/json", json.dumps(report).encode())
        except (ValueError, OSError, UnicodeError):
            self.send_body(400, "application/json", b'{"ok":false,"error":"Invalid upload"}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--files", type=Path, required=True)
    args = parser.parse_args()
    args.files.mkdir(parents=True, exist_ok=True)
    for name, data in FILES.items():
        target = args.files / name
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists() and target.read_bytes() != data:
            raise SystemExit(f"Refusing to replace an existing file: {target}")
        target.write_bytes(data)
    server = ThreadingHTTPServer(("127.0.0.1", 0), FixtureHandler)
    server.token = secrets.token_hex(18)
    print(f"Fixture: http://127.0.0.1:{server.server_port}/{server.token}", flush=True)
    print(f"Select files from: {args.files.resolve()}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
