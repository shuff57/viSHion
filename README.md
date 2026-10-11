# viSHion

Handwriting OCR in one HTML file, backed by a local llama.cpp vision server.

- `index.html` - upload handwritten pages (batch / multi-page), transcribe via `http://localhost:8188` (`/v1/chat/completions`), optional judge model on `:8189`, topic glossaries.
- `bakeoff/run.py` - CER bake-off of Qwen3-VL-8B, PaddleOCR-VL-1.6 and Nanonets-OCR2-3B on your own photos.

## Bake-off results (6 handwritten pages, RTX 4070 12 GB)

| Model | Mean CER | Notes |
|---|---|---|
| Nanonets-OCR2-3B (Q8) | 7.5% | best on multi-column math layout |
| Qwen3-VL-8B (Q8) | 13.2% | best on faint/messy handwriting |
| PaddleOCR-VL-1.6 | 22.4% | fastest (~1 s/page), drops words |

Small sample, hand-made ground truth: differences of a point or two are noise.

## Run

1. `llama-server -m model.gguf --mmproj mmproj.gguf -ngl 99 -c 8192 --port 8188`
2. Open `index.html` in a browser.

Bake-off: put `bin/llama/llama-server.exe` and `models/{qwen3vl,paddle,nanonets}/{model,mmproj}.gguf` under `bakeoff/`, add `truth.json`, set `OCR_PHOTOS`, then `python run.py qwen3vl paddle nanonets`.
