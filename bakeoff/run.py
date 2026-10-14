"""OCR bake-off: serve each model with llama-server, transcribe the test photos, save raw output + CER.
Run:  python -I run.py [model ...]     (default: all five)
"""
import base64, io, json, os, re, subprocess, sys, time, urllib.request
from pathlib import Path
from PIL import Image, ImageOps

ROOT = Path(__file__).parent
SERVER = next((ROOT / "bin").rglob("llama-server.exe"))
PHOTOS = Path(os.environ.get("OCR_PHOTOS", "photos"))
TRUTH = json.loads((ROOT / "truth.json").read_text(encoding="utf-8"))
PORT = 8188
LONG_EDGE = 1600

APP_PROMPT = ("Transcribe all the text in this image verbatim, exactly as written. Preserve line breaks, "
              "spelling, and abbreviations. Do not correct or normalize anything.")
NANONETS_PROMPT = ("Extract the text from the above document as if you were reading it naturally. Return the "
                   "tables in html format. Return the equations in LaTeX representation. If there is an image "
                   "in the document and image caption is not present, add a small description of the image "
                   "inside the <img></img> tag; otherwise, add the image caption inside <img></img>. Watermarks "
                   "should be wrapped in brackets. Ex: <watermark>OFFICIAL COPY</watermark>. Page numbers should "
                   "be wrapped in brackets. Ex: <page_number>14</page_number> or <page_number>9/22</page_number>. "
                   "Prefer using ☐ and ☑ for check boxes.")
# Each model lives in models/<dir>/ as model.gguf + mmproj.gguf (sources in README.md).
MODELS = {
    "qwen3vl":   {"dir": "qwen3vl",   "prompt": APP_PROMPT,       "extra": ["--temp", "0"]},  # Qwen3-VL-8B-Instruct Q8_0
    "qwen25vl":  {"dir": "qwen25vl",  "prompt": APP_PROMPT,       "extra": ["--temp", "0"]},  # ggml-org Qwen2.5-VL-7B-Instruct Q8_0
    "lumichats": {"dir": "lumichats", "prompt": APP_PROMPT,       "extra": ["--temp", "0"]},  # LumiChats v1.2 Q8_0 + Qwen2.5-VL projector
    "nanonets":  {"dir": "nanonets",  "prompt": NANONETS_PROMPT,  "extra": ["--temp", "0"]},  # mradermacher Nanonets-OCR2-3B Q8_0
    "paddle":    {"dir": "paddle",    "prompt": "OCR:",           "extra": ["--temp", "0"]},  # PaddleOCR-VL-1.6-GGUF
}


def prep(path: Path) -> str:
    im = ImageOps.exif_transpose(Image.open(path)).convert("RGB")
    s = LONG_EDGE / max(im.size)
    if s < 1:
        im = im.resize((round(im.width * s), round(im.height * s)), Image.LANCZOS)
    buf = io.BytesIO()
    im.save(buf, "JPEG", quality=90)
    return "data:image/jpeg;base64," + base64.b64encode(buf.getvalue()).decode()


def post(url, body, timeout=120):
    req = urllib.request.Request(url, json.dumps(body).encode(), {"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read())


def lev(a: str, b: str) -> int:
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i]
        for j, cb in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb)))
        prev = cur
    return prev[-1]


def norm(s: str) -> str:
    """Strip markup so formatting choices (markdown, LaTeX, html) are not scored as errors."""
    s = re.sub(r"<[^>]+>", " ", s)                      # html / <img> / <watermark>
    s = s.replace("\\frac", " ").replace("\\cdot", "*").replace("\\times", "*").replace("\\div", "/")
    s = s.replace("\\left", "").replace("\\right", "").replace("\\(", "").replace("\\)", "")
    s = s.replace("\\[", "").replace("\\]", "").replace("\\,", "")
    s = re.sub(r"[\$\\{}^_*#|`~]", "", s)
    s = s.replace("−", "-").replace("–", "-").replace("—", "-").replace("×", "*").replace("÷", "/")
    s = s.replace("’", "'").replace("“", '"').replace("”", '"')
    s = s.lower()
    return re.sub(r"[^a-z0-9+\-=/*().,']", "", s)       # drop whitespace and everything else


def run(name: str):
    cfg = MODELS[name]
    d = ROOT / "models" / cfg["dir"]
    log = open(ROOT / "out" / f"{name}-server.log", "w")
    cmd = [str(SERVER), "-m", str(d / "model.gguf"), "--mmproj", str(d / "mmproj.gguf"),
           "-ngl", "99", "-c", "8192", "--port", str(PORT), "--host", "127.0.0.1", "-np", "1"] + cfg["extra"]
    t_load = time.time()
    p = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT)
    try:
        for _ in range(240):
            try:
                if urllib.request.urlopen(f"http://127.0.0.1:{PORT}/health", timeout=2).status == 200:
                    break
            except Exception:
                if p.poll() is not None:
                    raise RuntimeError(f"{name}: server exited early, see out/{name}-server.log")
                time.sleep(1)
        else:
            raise RuntimeError(f"{name}: server never became healthy")
        load_s = time.time() - t_load
        rows = []
        for img, t in TRUTH.items():
            if img.startswith("_"):
                continue
            body = {"model": "ocr", "temperature": 0, "max_tokens": 1800, "messages": [{"role": "user", "content": [
                {"type": "image_url", "image_url": {"url": prep(PHOTOS / img)}},
                {"type": "text", "text": cfg["prompt"]}]}]}
            t0 = time.time()
            try:
                r = post(f"http://127.0.0.1:{PORT}/v1/chat/completions", body)
                out = r["choices"][0]["message"]["content"]
                toks = r.get("usage", {}).get("completion_tokens")
                err = None
            except Exception as e:
                out, toks, err = "", None, str(e)
            dt = time.time() - t0
            ref, hyp = norm(t["text"]), norm(out)
            cer = lev(ref, hyp) / max(1, len(ref))
            (ROOT / "out" / f"{name}__{Path(img).stem}.txt").write_text(out, encoding="utf-8")
            rows.append({"image": img, "cer": round(cer, 4), "seconds": round(dt, 1), "tokens": toks,
                         "chars": len(out), "error": err})
            print(f"{name:9s} {img:14s} CER {cer:6.1%}  {dt:5.1f}s  {toks} tok  {err or ''}", flush=True)
        return {"model": name, "load_seconds": round(load_s, 1), "rows": rows}
    finally:
        p.terminate()
        try:
            p.wait(15)
        except Exception:
            p.kill()
        log.close()
        time.sleep(3)


if __name__ == "__main__":
    names = sys.argv[1:] or list(MODELS)
    results = [run(n) for n in names]
    (ROOT / "out" / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
    print("\nMEAN CER")
    for r in results:
        v = [x["cer"] for x in r["rows"] if not x["error"]]
        print(f"  {r['model']:9s} {sum(v)/max(1,len(v)):6.1%}  ({len(v)}/{len(r['rows'])} ok)")
