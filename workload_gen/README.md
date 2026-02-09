# workload_gen

PyTorch-based workload generator that profiles model compute and estimates
communication volumes to emit `workload.json` (schema v2).

## Quick start

Start the API server (run from `workload_gen/`):

```
python3 -m workload_gen.server --port 3100
```

Generate a workload.json from CLI:

```
workload-gen generate \
  --model gpt2_med \
  --gpu NVIDIA_A100-PCIE-40GB \
  --mode train \
  --seq 2048 \
  --batch 8 \
  --dp 2 --tp 1 --pp 1 --pp-microbatch 1 \
  --dtype fp16 \
  --device-scale-mode max \
  --model-backend transformers \
  --device cuda \
  --out workload.json
```

The generator reads model configs from
`NeuSight/scripts/asplos/data/DLmodel_configs` and device configs from
`NeuSight/scripts/asplos/data/device_configs`.

Device scaling:
- When profiling on CUDA, the generator detects the local GPU name and scales
  compute time to the target GPU using `device_configs`. Use
  `--device-scale-mode` to pick `max/mean/compute/memory/none`.

Model backend:
- `transformers` builds a HuggingFace model from the local config JSON.
- `minimal` uses the lightweight internal Transformer.

## Megatron-LM integration

This project is structured to allow an optional Megatron-LM path, but the
current implementation uses a minimal PyTorch transformer to keep dependencies
light. If you install Megatron-LM locally, we can add a switch that swaps the
model builder to Megatron modules and keeps the hook-based profiler.
