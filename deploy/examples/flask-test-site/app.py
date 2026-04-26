from datetime import datetime, timezone
import os
import socket

from flask import Flask, jsonify, request


app = Flask(__name__)


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def build_request_summary():
    return {
        "method": request.method,
        "path": request.path,
        "query": request.args.to_dict(flat=False),
        "remote_addr": request.headers.get("X-Forwarded-For", request.remote_addr),
        "headers": {
            "host": request.headers.get("Host"),
            "user_agent": request.headers.get("User-Agent"),
            "x_forwarded_for": request.headers.get("X-Forwarded-For"),
            "x_forwarded_proto": request.headers.get("X-Forwarded-Proto"),
        },
    }


@app.get("/")
def index():
    return f"""
    <!doctype html>
    <html lang="en">
      <head>
        <meta charset="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <title>PingoraHub Test Site</title>
        <style>
          body {{
            font-family: ui-sans-serif, system-ui, sans-serif;
            margin: 0;
            background: linear-gradient(135deg, #0f172a, #134e4a);
            color: #eff6ff;
          }}
          main {{
            max-width: 900px;
            margin: 48px auto;
            padding: 32px;
          }}
          .card {{
            background: rgba(255, 255, 255, 0.08);
            border: 1px solid rgba(255, 255, 255, 0.12);
            border-radius: 20px;
            padding: 24px;
            backdrop-filter: blur(8px);
          }}
          code, pre {{
            font-family: ui-monospace, SFMono-Regular, monospace;
          }}
          a {{
            color: #99f6e4;
          }}
        </style>
      </head>
      <body>
        <main>
          <div class="card">
            <h1>PingoraHub Flask Test Site</h1>
            <p>This test site is running on port 8081.</p>
            <ul>
              <li>Hostname: <code>{socket.gethostname()}</code></li>
              <li>Server time (UTC): <code>{now_iso()}</code></li>
              <li>Listen address: <code>0.0.0.0:8081</code></li>
            </ul>
            <p>Useful endpoints:</p>
            <ul>
              <li><a href="/healthz">/healthz</a></li>
              <li><a href="/api/info">/api/info</a></li>
              <li><a href="/echo?hello=world">/echo?hello=world</a></li>
            </ul>
          </div>
        </main>
      </body>
    </html>
    """


@app.get("/healthz")
def healthz():
    return jsonify(
        {
            "service": "flask-test-site",
            "status": "ok",
            "hostname": socket.gethostname(),
            "time": now_iso(),
        }
    )


@app.get("/api/info")
def info():
    return jsonify(
        {
            "service": "flask-test-site",
            "hostname": socket.gethostname(),
            "time": now_iso(),
            "python": os.sys.version,
            "request": build_request_summary(),
        }
    )


@app.route("/echo", methods=["GET", "POST"])
def echo():
    body = request.get_json(silent=True)
    return jsonify(
        {
            "service": "flask-test-site",
            "time": now_iso(),
            "request": build_request_summary(),
            "json_body": body,
            "form": request.form.to_dict(flat=False),
        }
    )


if __name__ == "__main__":
    app.run(host="0.0.0.0", port=8081, debug=False)
