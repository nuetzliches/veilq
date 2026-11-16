"""Convert the signature detector of ADR-0048 to ONNX, weights unchanged.

The source is `mdefrance/yolos-tiny-signature-detection` (Apache-2.0) at a
fixed revision; the output is the `model.onnx` that nuetzliches publishes as
`nuetzliches/yolos-tiny-signature-detection-onnx` and veilq pins by its
SHA-256 (`crates/veilq-core/src/models.rs`). Input `pixel_values`
[1, 3, 640, 640]; outputs `logits` [1, 100, 2] (signature, nothing) and
`pred_boxes` [1, 100, 4] (centre x, centre y, width, height as shares).

    python -m venv venv
    venv/Scripts/python -m pip install torch==2.15.0 --index-url https://download.pytorch.org/whl/cpu
    venv/Scripts/python -m pip install transformers==4.57.6 onnx huggingface_hub
    venv/Scripts/python -I scripts/export-signature-model.py <empty output directory>
"""

import hashlib
import sys
from pathlib import Path

import torch
from huggingface_hub import snapshot_download
from transformers import AutoModelForObjectDetection

REPO = "mdefrance/yolos-tiny-signature-detection"
REVISION = "b788cf7376464453c3dea1e5ee8589adcaa0c634"


class Outputs(torch.nn.Module):
    """The model with plain tensors out: logits and boxes."""

    def __init__(self, model):
        super().__init__()
        self.model = model

    def forward(self, pixel_values):
        out = self.model(pixel_values=pixel_values)
        return out.logits, out.pred_boxes


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    source = snapshot_download(
        REPO,
        revision=REVISION,
        local_dir=out / "source",
        allow_patterns=["config.json", "model.safetensors", "preprocessor_config.json", "README.md"],
    )
    model = AutoModelForObjectDetection.from_pretrained(source).eval()
    target = out / "model.onnx"
    torch.onnx.export(
        Outputs(model),
        (torch.zeros(1, 3, 640, 640),),
        str(target),
        input_names=["pixel_values"],
        output_names=["logits", "pred_boxes"],
        opset_version=17,
        dynamo=False,
    )
    print(target, hashlib.sha256(target.read_bytes()).hexdigest())


if __name__ == "__main__":
    main()
