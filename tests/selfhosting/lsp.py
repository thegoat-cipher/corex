#!/usr/bin/env python3
"""Deterministic tests for the CoreX language server (`corexc lsp`).

Launches the server, speaks framed JSON-RPC over stdio, and asserts responses:
initialize capabilities, publishDiagnostics, hover, definition, references,
rename, completion, formatting, didChange/didClose, and unknown-method handling.

    tests/selfhosting/lsp.py [compiler] [--oracle corex-lsp]

`--oracle` additionally runs the same request sequence against the Rust server
and compares the protocol-visible results (capabilities, diagnostic codes,
severities and ranges, edit ranges, completion labels/details) ignoring JSON
key order and diagnostic prose.
"""

import json
import os
import select
import subprocess
import sys

SOURCE = """fn helper() -> i32 {
  return 1;
}
fn main() -> i32 {
  let x = missing;
  return helper();
}
"""

FORMATTED = """fn helper() -> i32 {
    return 1;
}
fn main() -> i32 {
    let x = missing;
    return helper();
}
"""

URI = "file:///tmp/corexc-lsp-test.cx"
KEYWORDS = (
    "fn let mut if else while loop for do switch case default break continue "
    "return unsafe struct union enum typedef static const extern pub mod module "
    "as true false null sizeof alignof offsetof goto cstr str vec i8 i16 i32 i64 "
    "u8 u16 u32 u64 usize isize bool char f32 f64"
).split()


class LspError(Exception):
    pass


class Session:
    def __init__(self, exe):
        self.proc = subprocess.Popen(
            [exe, "lsp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            bufsize=0,
        )
        self.next_id = 1
        self.pending = []

    def send(self, message):
        body = json.dumps(message).encode()
        self.proc.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
        self.proc.stdin.flush()

    def _readline(self):
        ready, _, _ = select.select([self.proc.stdout], [], [], 10.0)
        if not ready:
            raise LspError("timeout waiting for server output")
        line = self.proc.stdout.readline()
        if not line:
            raise LspError("server closed stdout")
        return line

    def recv(self):
        length = None
        while True:
            line = self._readline()
            if line in (b"\r\n", b"\n"):
                break
            text = line.decode("utf-8", "replace").strip()
            if text.lower().startswith("content-length:"):
                length = int(text.split(":", 1)[1].strip())
        if length is None:
            raise LspError("frame without Content-Length")
        body = b""
        while len(body) < length:
            chunk = self.proc.stdout.read(length - len(body))
            if not chunk:
                raise LspError("truncated frame body")
            body += chunk
        return json.loads(body.decode("utf-8"))

    def request(self, method, params):
        ident = self.next_id
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": ident, "method": method, "params": params})
        while True:
            message = self.recv()
            if message.get("id") == ident and "method" not in message:
                if "error" in message:
                    raise LspError("error response: %r" % (message["error"],))
                return message.get("result")
            self.pending.append(message)

    def notify(self, method, params):
        self.send({"jsonrpc": "2.0", "method": method, "params": params})

    def next_notification(self):
        if self.pending:
            return self.pending.pop(0)
        return self.recv()

    def publish(self, uri):
        """Return the diagnostics array from the next publishDiagnostics."""
        message = self.next_notification()
        if message.get("method") != "textDocument/publishDiagnostics":
            raise LspError("expected publishDiagnostics, got %r" % (message,))
        params = message.get("params", {})
        if params.get("uri") != uri:
            raise LspError("diagnostics for %r, expected %r" % (params.get("uri"), uri))
        return params.get("diagnostics", [])

    def close(self):
        try:
            self.send({"jsonrpc": "2.0", "id": self.next_id, "method": "shutdown"})
            self.req_seen = self.recv()
            self.send({"jsonrpc": "2.0", "method": "exit"})
        finally:
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
        return self.proc.returncode


passed = 0
failed = 0


def check(condition, label, detail=""):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print("FAIL: %s%s" % (label, (": " + detail) if detail else ""), file=sys.stderr)


def positions(span):
    return (
        span["start"]["line"],
        span["start"]["character"],
        span["end"]["line"],
        span["end"]["character"],
    )


def run_sequence(exe):
    """Run the fixed protocol sequence; return a record of observable results."""
    s = Session(exe)
    out = {}

    init = s.request("initialize", {"capabilities": {}})
    out["capabilities"] = init["capabilities"]
    out["server_name"] = init["serverInfo"]["name"]

    s.notify("initialized", {})
    s.notify(
        "textDocument/didOpen",
        {
            "textDocument": {
                "uri": URI,
                "languageId": "corex",
                "version": 1,
                "text": SOURCE,
            }
        },
    )
    diags = s.publish(URI)
    out["diagnostics"] = diags

    out["hover"] = s.request(
        "textDocument/hover",
        {"textDocument": {"uri": URI}, "position": {"line": 5, "character": 12}},
    )
    out["hover_missing"] = s.request(
        "textDocument/hover",
        {"textDocument": {"uri": URI}, "position": {"line": 2, "character": 0}},
    )
    out["definition"] = s.request(
        "textDocument/definition",
        {"textDocument": {"uri": URI}, "position": {"line": 5, "character": 12}},
    )
    out["references"] = s.request(
        "textDocument/references",
        {"textDocument": {"uri": URI}, "position": {"line": 5, "character": 12}},
    )
    out["rename"] = s.request(
        "textDocument/rename",
        {
            "textDocument": {"uri": URI},
            "position": {"line": 5, "character": 12},
            "newName": "assist",
        },
    )
    out["rename_invalid"] = s.request(
        "textDocument/rename",
        {
            "textDocument": {"uri": URI},
            "position": {"line": 5, "character": 12},
            "newName": "1bad",
        },
    )
    out["formatting"] = s.request(
        "textDocument/formatting",
        {
            "textDocument": {"uri": URI},
            "options": {"tabSize": 4, "insertSpaces": True},
        },
    )
    out["completion"] = s.request(
        "textDocument/completion",
        {"textDocument": {"uri": URI}, "position": {"line": 2, "character": 0}},
    )

    # Full-document sync: the last content change wins.
    s.notify(
        "textDocument/didChange",
        {
            "textDocument": {"uri": URI, "version": 2},
            "contentChanges": [
                {"text": "fn main() -> i32 { return nope; }\n"},
                {"text": "fn main() -> i32 { return 0; }\n"},
            ],
        },
    )
    out["changed_diagnostics"] = s.publish(URI)

    out["unknown"] = s.request("textDocument/nonexistent", {"textDocument": {"uri": URI}})

    s.notify("textDocument/didClose", {"textDocument": {"uri": URI}})
    out["closed_diagnostics"] = s.publish(URI)

    out["exit_code"] = s.close()
    return out


def verify(rec):
    caps = rec["capabilities"]
    check(caps["textDocumentSync"] == 1, "initialize textDocumentSync")
    check(caps["completionProvider"]["triggerCharacters"] == [".", ":"], "completion triggers")
    for key in (
        "definitionProvider",
        "referencesProvider",
        "renameProvider",
        "documentFormattingProvider",
        "hoverProvider",
    ):
        check(caps[key] is True, "initialize %s" % key)
    check(rec["server_name"] == "corexc-lsp", "serverInfo.name")

    diags = rec["diagnostics"]
    check(len(diags) == 2, "diagnostic count", repr(diags))
    if len(diags) == 2:
        warn, err = diags
        check(warn["code"] == "W0001", "lint code", repr(warn))
        check(warn["severity"] == 2, "lint severity")
        check("unused variable" in warn["message"], "lint message", warn["message"])
        check(positions(warn["range"]) == (4, 6, 4, 7), "lint range", repr(warn["range"]))
        check(err["code"] == "E0201", "error code", repr(err))
        check(err["severity"] == 1, "error severity")
        check("undefined name" in err["message"], "error message", err["message"])
        check(positions(err["range"]) == (4, 10, 4, 17), "error range", repr(err["range"]))

    check(
        rec["hover"]["contents"]["value"] == "```corex\nfn helper() -> i32\n```",
        "hover signature",
        repr(rec["hover"]),
    )
    check(rec["hover_missing"] is None, "hover on punctuation is null")
    check(
        rec["definition"]["uri"] == URI and positions(rec["definition"]["range"]) == (0, 3, 0, 9),
        "definition points at the function name",
        repr(rec["definition"]),
    )
    refs = rec["references"]
    check(len(refs) == 2, "reference count", repr(refs))
    check(
        [positions(r["range"]) for r in refs] == [(0, 3, 0, 9), (5, 9, 5, 15)],
        "reference ranges",
        repr(refs),
    )
    rename = rec["rename"]["changes"][URI]
    check(len(rename) == 2, "rename edit count", repr(rename))
    check(all(e["newText"] == "assist" for e in rename), "rename text")
    check(rec["rename_invalid"] is None, "rename rejects an invalid identifier")

    edits = rec["formatting"]
    check(len(edits) == 1, "formatting edit count")
    check(edits[0]["newText"] == FORMATTED, "formatting text", repr(edits[0]["newText"]))
    check(positions(edits[0]["range"]) == (0, 0, 7, 0), "formatting range", repr(edits[0]["range"]))

    items = rec["completion"]
    labels = [i["label"] for i in items]
    missing = [k for k in KEYWORDS if k not in labels]
    check(not missing, "completion keywords", repr(missing))
    helper = [i for i in items if i["label"] == "helper"]
    check(
        len(helper) == 1 and helper[0]["detail"] == "fn helper() -> i32" and helper[0]["kind"] == 3,
        "completion includes the function",
        repr(helper),
    )
    local = [i for i in items if i["label"] == "x"]
    check(
        len(local) == 1 and local[0]["detail"] == "let x",
        "completion includes the local",
        repr(local),
    )

    check(rec["changed_diagnostics"] == [], "didChange clears diagnostics", repr(rec["changed_diagnostics"]))
    check(rec["unknown"] is None, "unknown methods return null")
    check(rec["closed_diagnostics"] == [], "didClose clears diagnostics")
    check(rec["exit_code"] == 0, "exit status", repr(rec["exit_code"]))


def normalized(rec):
    """Protocol-visible evidence, independent of JSON key order and prose."""
    def norm_diags(items):
        return [
            (
                d.get("code"),
                d.get("severity"),
                positions(d["range"]),
            )
            for d in items
        ]

    caps = rec["capabilities"]
    rename = rec["rename"]["changes"][URI]
    return {
        "sync": caps["textDocumentSync"],
        "triggers": caps["completionProvider"]["triggerCharacters"],
        "providers": sorted(k for k, v in caps.items() if v is True),
        "diags": norm_diags(rec["diagnostics"]),
        "hover": rec["hover"]["contents"]["kind"],
        "hover_value": rec["hover"]["contents"]["value"],
        "definition": positions(rec["definition"]["range"]),
        "references": [positions(r["range"]) for r in rec["references"]],
        "rename": [positions(e["range"]) for e in rename],
        "format": rec["formatting"][0]["newText"],
        "completion": sorted((i["label"], i["kind"], i.get("detail")) for i in rec["completion"]),
        "changed": norm_diags(rec["changed_diagnostics"]),
        "closed": norm_diags(rec["closed_diagnostics"]),
        "exit": rec["exit_code"],
    }


def main(argv):
    compiler = None
    oracle = None
    i = 1
    while i < len(argv):
        if argv[i] == "--oracle":
            if i + 1 >= len(argv):
                print("--oracle needs a path", file=sys.stderr)
                return 2
            oracle = argv[i + 1]
            i += 2
            continue
        compiler = argv[i]
        i += 1
    root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    compiler = compiler or os.path.join(root, "build", "stage2", "corexc")
    if not os.path.isfile(compiler) or not os.access(compiler, os.X_OK):
        print("missing compiler: %s" % compiler, file=sys.stderr)
        return 2

    record = run_sequence(compiler)
    verify(record)

    if oracle:
        oracle_record = run_sequence(oracle)
        mine = normalized(record)
        theirs = normalized(oracle_record)
        for key in mine:
            check(mine[key] == theirs[key], "oracle parity: %s" % key, "%r != %r" % (mine[key], theirs[key]))

    print("lsp tests: %d passed, %d failed" % (passed, failed))
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
