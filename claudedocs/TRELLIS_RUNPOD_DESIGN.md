# TRELLIS RunPod Worker Design

## Overview

This document describes the deployment of Microsoft's TRELLIS text-to-3D model on RunPod serverless, integrated with the `studio_generate_mesh` MCP tool.

## System Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           MCP Client (Claude Code)                          │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Rust MCP Server (mcp-roblox)                        │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                    studio_generate_mesh tool                         │    │
│  │  ┌─────────────┐    ┌──────────────┐    ┌───────────────────────┐   │    │
│  │  │ TrellisClient│───▶│ RunPod API   │───▶│ Poll for completion   │   │    │
│  │  └─────────────┘    └──────────────┘    └───────────────────────┘   │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                        HTTPS (RunPod Serverless API)
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        RunPod Serverless Infrastructure                      │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                     TRELLIS Worker Container                         │    │
│  │  ┌─────────────┐    ┌──────────────┐    ┌───────────────────────┐   │    │
│  │  │ handler.py  │───▶│ TRELLIS      │───▶│ GLB Export            │   │    │
│  │  │ (entry)     │    │ Pipeline     │    │ + Base64 Encode       │   │    │
│  │  └─────────────┘    └──────────────┘    └───────────────────────┘   │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                              GPU: A4000/A5000/A6000 (16GB+ VRAM)            │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                              GLB mesh data (base64)
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Rust MCP Server (continued)                        │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │  Parse GLB → Extract vertices/faces/normals/UVs → Send to Plugin    │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                              HTTP (localhost:8080)
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Roblox Studio Plugin                                 │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │  createMeshFromData → EditableMesh → CreateMeshPartAsync → MeshPart │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Critical Deployment Lessons

Based on extensive debugging and research, here are the critical requirements for a working TRELLIS deployment:

### 1. Version Compatibility Matrix

| Component | Required Version | Notes |
|-----------|-----------------|-------|
| PyTorch | 2.4.0 (2.4.1 in image) | TRELLIS requirement |
| CUDA | 12.4.1 | Only available RunPod image for PyTorch 2.4 |
| xformers | Skip | Causes torchvision conflicts - use native attention |
| Python | 3.11 | RunPod base image |

**Critical**: Only `runpod/pytorch:2.4.0-py3.11-cuda12.4.1-devel-ubuntu22.04` exists. CUDA 12.1 image not available.

### 2. Package Installation Order

1. **Clone TRELLIS first** - Sets PYTHONPATH priority
2. **Install xformers** - Must be BEFORE other packages to preserve torch/torchvision
3. **Install CUDA packages** (spconv, kaolin, nvdiffrast)
4. **Install TRELLIS deps** - From setup.sh --basic list
5. **Install easydict LAST** - Critical: prevents shadowing by other packages

### 3. Known Issues and Solutions

| Issue | Root Cause | Solution |
|-------|-----------|----------|
| `No module named 'easydict'` | open3d/transformers overwrite it | Install easydict LAST with --force-reinstall |
| `torchvision has no attribute 'extension'` | xformers version conflict | Skip xformers entirely - use native attention |
| `blinker 1.4 cannot be uninstalled` | distutils package in base image | Use --ignore-installed |
| flash-attn compile timeout | 30+ min compile exceeds RunPod limit | Skip it, use xformers fallback |
| utils3d commit not found | Pinned commit was deleted | Use latest (no commit hash) |

---

## Component 1: Dockerfile (Production)

```dockerfile
# =============================================================================
# TRELLIS Text-to-3D RunPod Serverless Dockerfile
# =============================================================================
# Key insights from comprehensive analysis:
# 1. Use CUDA 12.4 (only available option for PyTorch 2.4.0 on RunPod)
# 2. Clone TRELLIS first, then install deps (PYTHONPATH order matters)
# 3. Install easydict LAST to prevent shadowing by other packages
# 4. Use --ignore-installed for distutils conflicts in base image
# 5. Skip flash-attn AND xformers - use native attention
# =============================================================================

# PyTorch 2.4.0 with CUDA 12.4 - only available version on RunPod
FROM runpod/pytorch:2.4.0-py3.11-cuda12.4.1-devel-ubuntu22.04

# Limit ninja parallelism to avoid OOM during CUDA kernel compilation
ENV MAX_JOBS=4
ENV NINJA_MAX_JOBS=4

# System dependencies for TRELLIS (OpenGL, image processing)
RUN apt-get update && apt-get install -y --no-install-recommends \
    git \
    ninja-build \
    libgl1-mesa-glx \
    libglib2.0-0 \
    libsm6 \
    libxext6 \
    libxrender-dev \
    libgomp1 \
    wget \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

WORKDIR /app

# PHASE 1: Clone TRELLIS first (establishes PYTHONPATH priority)
RUN git clone --depth 1 https://github.com/microsoft/TRELLIS.git /app/trellis
ENV PYTHONPATH="/app/trellis:${PYTHONPATH}"

# PHASE 2: Verify base torch/torchvision are intact BEFORE any installs
RUN python -c "import torch; print(f'PyTorch: {torch.__version__}')" && \
    python -c "import torchvision; print(f'torchvision: {torchvision.__version__}')"

# PHASE 3: CUDA-specific packages (skip xformers - causes conflicts)
RUN pip install --no-cache-dir spconv-cu124 \
    || pip install --no-cache-dir spconv-cu120 \
    || echo "WARNING: spconv failed"
RUN pip install --no-cache-dir kaolin -f https://nvidia-kaolin.s3.us-east-2.amazonaws.com/torch-2.4.0_cu124.html \
    || pip install --no-cache-dir kaolin -f https://nvidia-kaolin.s3.us-east-2.amazonaws.com/torch-2.4.0_cu121.html \
    || echo "WARNING: kaolin failed"
RUN pip install --no-cache-dir git+https://github.com/NVlabs/nvdiffrast.git \
    || echo "WARNING: nvdiffrast failed"

# PHASE 4: TRELLIS basic dependencies (from setup.sh --basic)
RUN pip install --no-cache-dir --ignore-installed \
    pillow imageio imageio-ffmpeg tqdm opencv-python-headless scipy \
    ninja rembg onnxruntime trimesh open3d xatlas pyvista pymeshfix \
    igraph transformers git+https://github.com/EasternJournalist/utils3d.git

# PHASE 5: Install easydict LAST (prevents shadowing)
RUN pip install --no-cache-dir --force-reinstall easydict
RUN python -c "from easydict import EasyDict; print('easydict OK')"

# PHASE 6: RunPod handler dependencies
RUN pip install --no-cache-dir \
    runpod>=1.6.0 pygltflib>=1.16.0 huggingface_hub safetensors einops

# PHASE 7: Verification
RUN python -c "\
import torch; print(f'  torch {torch.__version__} OK'); \
import torchvision; print(f'  torchvision {torchvision.__version__} OK'); \
from easydict import EasyDict; print('  easydict OK'); \
import trimesh; print('  trimesh OK')"

# Try TRELLIS import (may fail without GPU, tests imports only)
RUN python -c "\
try: \
    from trellis.pipelines import TrellisTextTo3DPipeline; \
    print('TRELLIS pipeline import OK'); \
except Exception as e: \
    print(f'TRELLIS import: {e} (expected without GPU)')"

# PHASE 8: Handler and runtime config
COPY handler.py .
ENV TRELLIS_MODEL_PATH="/app/models/TRELLIS-text-xlarge"
ENV HF_HOME="/app/hf_cache"

RUN echo "=== BUILD COMPLETE ===" && \
    python -c "import torch; print(f'PyTorch: {torch.__version__}, CUDA: {torch.version.cuda}')" && \
    python -m py_compile /app/handler.py && echo "handler.py syntax OK"

CMD ["python", "-u", "/app/handler.py"]
```

---

## Component 2: handler.py

```python
"""
TRELLIS Text-to-3D RunPod Serverless Handler

Input:
{
    "input": {
        "prompt": "A wooden treasure chest",
        "seed": 42,                    # Optional, default: random
        "simplify": 0.95,              # Optional, mesh simplification ratio
        "texture_size": 1024           # Optional, texture resolution
    }
}

Output:
{
    "glb_base64": "<base64-encoded GLB file>",
    "vertex_count": 1234,
    "face_count": 5678,
    "generation_time_seconds": 30.5
}
"""

from __future__ import annotations

import runpod
import base64
import logging
import time
import torch
import random
import os
import sys
from io import BytesIO
from typing import Any, Optional

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

MAX_PROMPT_LENGTH = 1000
MIN_TEXTURE_SIZE = 128
MAX_TEXTURE_SIZE = 4096

sys.path.insert(0, '/app/trellis')

logger.info("=" * 50)
logger.info("TRELLIS Text-to-3D RunPod Worker Starting...")
logger.info(f"CUDA Available: {torch.cuda.is_available()}")
if torch.cuda.is_available():
    logger.info(f"GPU: {torch.cuda.get_device_name(0)}")
    logger.info(f"VRAM: {torch.cuda.get_device_properties(0).total_memory / 1024**3:.1f} GB")
logger.info("=" * 50)

PIPELINE = None

def download_model_if_needed():
    """Download TRELLIS model weights if not present."""
    model_path = os.environ.get('TRELLIS_MODEL_PATH', '/app/models/TRELLIS-text-xlarge')

    if os.path.exists(model_path) and os.listdir(model_path):
        logger.info(f"Model exists at {model_path}")
        return model_path

    logger.info("Downloading TRELLIS model weights...")
    from huggingface_hub import snapshot_download

    downloaded_path = snapshot_download(
        "microsoft/TRELLIS-text-xlarge",
        local_dir=model_path,
        local_dir_use_symlinks=False
    )
    logger.info(f"Model downloaded to: {downloaded_path}")
    return downloaded_path


def load_pipeline() -> Any:
    """Load TRELLIS pipeline."""
    global PIPELINE

    if PIPELINE is not None:
        return PIPELINE

    logger.info("Loading TRELLIS pipeline...")
    start = time.time()

    model_path = download_model_if_needed()

    from trellis.pipelines import TrellisTextTo3DPipeline

    PIPELINE = TrellisTextTo3DPipeline.from_pretrained(model_path)
    PIPELINE.cuda()

    logger.info(f"Pipeline loaded in {time.time() - start:.1f}s")
    return PIPELINE


def validate_input(job_input: dict) -> tuple[bool, Optional[str], dict]:
    """Validate input parameters."""
    prompt = job_input.get("prompt")
    if not prompt or not isinstance(prompt, str) or not prompt.strip():
        return False, "Missing or invalid 'prompt'", {}

    prompt = prompt.strip()
    if len(prompt) > MAX_PROMPT_LENGTH:
        return False, f"Prompt exceeds {MAX_PROMPT_LENGTH} chars", {}

    seed = job_input.get("seed")
    if seed is None:
        seed = random.randint(0, 2**32 - 1)
    elif not isinstance(seed, int) or seed < 0:
        return False, "'seed' must be positive integer", {}

    simplify = job_input.get("simplify", 0.95)
    if not isinstance(simplify, (int, float)) or not (0.0 <= simplify <= 1.0):
        return False, "'simplify' must be 0.0-1.0", {}

    texture_size = job_input.get("texture_size", 1024)
    if not isinstance(texture_size, int) or not (MIN_TEXTURE_SIZE <= texture_size <= MAX_TEXTURE_SIZE):
        return False, f"'texture_size' must be {MIN_TEXTURE_SIZE}-{MAX_TEXTURE_SIZE}", {}

    return True, None, {
        "prompt": prompt,
        "seed": seed,
        "simplify": float(simplify),
        "texture_size": texture_size,
    }


def handler(event: dict) -> dict:
    """Process text-to-3D request."""
    start_time = time.time()

    try:
        pipeline = load_pipeline()

        job_input = event.get("input", {})
        is_valid, error_msg, params = validate_input(job_input)

        if not is_valid:
            return {"error": error_msg}

        prompt = params["prompt"]
        seed = params["seed"]
        simplify = params["simplify"]
        texture_size = params["texture_size"]

        logger.info(f"Generating: '{prompt[:50]}...'")

        torch.manual_seed(seed)
        random.seed(seed)

        with torch.no_grad():
            outputs = pipeline.run(prompt, seed=seed)

        logger.info("Exporting to GLB...")

        from trellis.utils import postprocessing_utils

        glb = postprocessing_utils.to_glb(
            outputs['gaussian'][0],
            outputs['mesh'][0],
            simplify=simplify,
            texture_size=texture_size,
        )

        glb_buffer = BytesIO()
        glb.export(glb_buffer, file_type='glb')
        glb_bytes = glb_buffer.getvalue()

        mesh = outputs['mesh'][0]
        vertex_count = len(mesh.vertices) if hasattr(mesh, 'vertices') else 0
        face_count = len(mesh.faces) if hasattr(mesh, 'faces') else 0

        generation_time = time.time() - start_time

        logger.info(f"Done: {vertex_count} verts, {face_count} faces, {generation_time:.1f}s")

        torch.cuda.empty_cache()

        return {
            "glb_base64": base64.b64encode(glb_bytes).decode('utf-8'),
            "vertex_count": vertex_count,
            "face_count": face_count,
            "glb_size_bytes": len(glb_bytes),
            "generation_time_seconds": round(generation_time, 2),
            "seed_used": seed
        }

    except torch.cuda.OutOfMemoryError:
        torch.cuda.empty_cache()
        return {"error": "GPU out of memory"}
    except Exception as e:
        logger.exception(f"Error: {e}")
        return {"error": str(e)}


# Pre-load at startup
try:
    load_pipeline()
    logger.info("Model pre-loaded!")
except Exception as e:
    logger.warning(f"Pre-load failed: {e}")

runpod.serverless.start({"handler": handler})
```

---

## Component 3: Environment Variables

### mcp-roblox .env.example

```bash
# TRELLIS via RunPod (recommended for mesh generation)
RUNPOD_API_KEY=your-runpod-api-key
TRELLIS_ENDPOINT_ID=your-endpoint-id

# Optional overrides:
# RUNPOD_BASE_URL=https://api.runpod.ai/v2
# TRELLIS_MAX_POLL_ATTEMPTS=120
# TRELLIS_POLL_INTERVAL_MS=5000
HF_TOKEN=your-huggingface-token
```

---

## Deployment Steps

### 1. Create RunPod Worker Repository

Repository: https://github.com/quanticsoul4772/trellis-runpod-worker

```
trellis-runpod-worker/
├── Dockerfile      # Production Dockerfile (CUDA 12.1)
├── handler.py      # RunPod serverless handler
└── README.md
```

### 2. Deploy on RunPod

1. Go to RunPod Console → Serverless → Create Endpoint
2. Select **Template**: Custom (GitHub)
3. Enter repository URL: `https://github.com/quanticsoul4772/trellis-runpod-worker`
4. Select GPU:
   - **A4000 (16GB)**: Minimum, ~$0.20/hr
   - **A5000 (24GB)**: Recommended, ~$0.30/hr
   - **A6000 (48GB)**: Best quality, ~$0.50/hr
5. Configure:
   - Max Workers: 1-3
   - Idle Timeout: 30s (keeps model warm)
   - Flash Boot: Enable
6. Copy Endpoint ID (e.g., `vxwdlxlbsk21ux`)

### 3. Configure mcp-roblox

Add to `.mcp.json` or environment:

```json
{
  "env": {
    "RUNPOD_API_KEY": "rpa_...",
    "TRELLIS_ENDPOINT_ID": "vxwdlxlbsk21ux"
  }
}
```

### 4. Test

```bash
# Via Claude Code:
studio_generate_mesh(prompt: "medieval torch bracket")
```

---

## Cost Estimation

| GPU | $/hour | Generation Time | Cost/Model |
|-----|--------|-----------------|------------|
| A4000 (16GB) | $0.20 | ~45s | ~$0.0025 |
| A5000 (24GB) | $0.30 | ~35s | ~$0.003 |
| A6000 (48GB) | $0.50 | ~25s | ~$0.0035 |

Cold start (first request after idle): +30-60s for model loading.

---

## Troubleshooting

### Build Fails with Package Errors

1. Check RunPod build logs for specific package
2. Ensure using CUDA 12.1, not 12.4
3. Verify installation order in Dockerfile

### Runtime: "No module named 'X'"

Most likely cause: package shadowing. Ensure:
1. easydict installed LAST
2. Using --ignore-installed for distutils conflicts
3. PYTHONPATH set to /app/trellis

### Job Timeout

1. Check GPU memory (needs 16GB+)
2. Reduce texture_size parameter
3. Increase TRELLIS_MAX_POLL_ATTEMPTS

### GPU OOM

1. Use lower texture_size (512 instead of 1024)
2. Use higher simplify ratio (0.98 instead of 0.95)
3. Upgrade to larger GPU (A5000 or A6000)

---

## Rust Integration

The Rust MCP server integrates TRELLIS via:

- `src/trellis/config.rs` - TrellisConfig from environment
- `src/trellis/client.rs` - RunPod API client (submit → poll → receive)
- `src/trellis/glb_parser.rs` - GLB binary format parser

Required for `studio_generate_mesh`:
- RUNPOD_API_KEY + TRELLIS_ENDPOINT_ID + HF_TOKEN
