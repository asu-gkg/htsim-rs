from dataclasses import dataclass
import time
from typing import Dict, Iterable, List, Optional, Tuple

import torch


@dataclass
class LayerProfile:
    name: str
    fw_ms: float
    bw_ms: float
    output_shape: Tuple[int, ...]


@dataclass
class ExtraProfile:
    name: str
    fw_ms: float
    bw_ms: float


@dataclass
class ProfileResult:
    layers: List[LayerProfile]
    prologue: Optional[ExtraProfile]
    epilogue: Optional[ExtraProfile]


class ModuleTimer:
    def __init__(self, use_cuda: bool):
        self.use_cuda = use_cuda
        self._fw_start: Dict[str, object] = {}
        self._bw_start: Dict[str, object] = {}
        self.fw_ms: Dict[str, float] = {}
        self.bw_ms: Dict[str, float] = {}
        self.output_shapes: Dict[str, Tuple[int, ...]] = {}

    def reset(self) -> None:
        self._fw_start.clear()
        self._bw_start.clear()
        self.fw_ms.clear()
        self.bw_ms.clear()
        self.output_shapes.clear()

    def _start(self, store: Dict[str, object], name: str) -> None:
        if self.use_cuda:
            event = torch.cuda.Event(enable_timing=True)
            event.record()
            store[name] = event
        else:
            store[name] = time.perf_counter()

    def _end(self, store: Dict[str, object], acc: Dict[str, float], name: str) -> None:
        start = store.pop(name, None)
        if start is None:
            return
        if self.use_cuda:
            end = torch.cuda.Event(enable_timing=True)
            end.record()
            torch.cuda.synchronize()
            elapsed = start.elapsed_time(end)
        else:
            elapsed = (time.perf_counter() - start) * 1000.0
        acc[name] = acc.get(name, 0.0) + float(elapsed)

    def start_fw(self, name: str) -> None:
        self._start(self._fw_start, name)

    def end_fw(self, name: str, output: object) -> None:
        if name not in self.output_shapes:
            shape = _extract_shape(output)
            if shape:
                self.output_shapes[name] = shape
        self._end(self._fw_start, self.fw_ms, name)

    def start_bw(self, name: str) -> None:
        self._start(self._bw_start, name)

    def end_bw(self, name: str) -> None:
        self._end(self._bw_start, self.bw_ms, name)


def _extract_shape(output: object) -> Optional[Tuple[int, ...]]:
    if isinstance(output, torch.Tensor):
        return tuple(output.shape)
    if hasattr(output, "last_hidden_state") and isinstance(output.last_hidden_state, torch.Tensor):
        return tuple(output.last_hidden_state.shape)
    if isinstance(output, (list, tuple)) and output:
        for item in output:
            if isinstance(item, torch.Tensor):
                return tuple(item.shape)
    return None


def _extract_tensor(output: object) -> Optional[torch.Tensor]:
    if isinstance(output, torch.Tensor):
        return output
    if hasattr(output, "last_hidden_state") and isinstance(output.last_hidden_state, torch.Tensor):
        return output.last_hidden_state
    if isinstance(output, (list, tuple)) and output:
        for item in output:
            if isinstance(item, torch.Tensor):
                return item
    return None


def _attach_hooks(
    timer: ModuleTimer,
    modules: Iterable[Tuple[str, torch.nn.Module]],
    mode: str,
) -> List[torch.utils.hooks.RemovableHandle]:
    handles: List[torch.utils.hooks.RemovableHandle] = []

    for name, module in modules:
        handles.append(
            module.register_forward_pre_hook(
                lambda _m, _inp, name=name: timer.start_fw(name)
            )
        )
        handles.append(
            module.register_forward_hook(
                lambda _m, _inp, out, name=name: timer.end_fw(name, out)
            )
        )
        if mode == "train":
            handles.append(
                module.register_full_backward_pre_hook(
                    lambda _m, _grad_out, name=name: timer.start_bw(name)
                )
            )
            handles.append(
                module.register_full_backward_hook(
                    lambda _m, _grad_in, _grad_out, name=name: timer.end_bw(name)
                )
            )
    return handles


def profile_model(
    model: torch.nn.Module,
    input_ids: torch.Tensor,
    layer_modules: List[Tuple[str, torch.nn.Module]],
    prologue_modules: List[Tuple[str, torch.nn.Module]],
    epilogue_modules: List[Tuple[str, torch.nn.Module]],
    mode: str,
    warmup_steps: int,
    measure_steps: int,
) -> ProfileResult:
    use_cuda = input_ids.is_cuda
    timer = ModuleTimer(use_cuda=use_cuda)
    all_modules = prologue_modules + layer_modules + epilogue_modules
    handles = _attach_hooks(timer, all_modules, mode)

    def run_once() -> None:
        model.zero_grad(set_to_none=True)
        if mode == "train":
            output = model(input_ids)
            tensor = _extract_tensor(output)
            if tensor is None:
                raise RuntimeError("unable to extract tensor output for backward")
            loss = tensor.float().sum()
            loss.backward()
        else:
            with torch.no_grad():
                _ = model(input_ids)

    try:
        for _ in range(max(0, warmup_steps)):
            run_once()
        timer.reset()
        for _ in range(max(1, measure_steps)):
            run_once()
        if measure_steps > 1:
            for name in list(timer.fw_ms.keys()):
                timer.fw_ms[name] = timer.fw_ms[name] / measure_steps
            for name in list(timer.bw_ms.keys()):
                timer.bw_ms[name] = timer.bw_ms[name] / measure_steps
    finally:
        for handle in handles:
            handle.remove()

    layers: List[LayerProfile] = []
    for name, _module in layer_modules:
        shape = timer.output_shapes.get(name, ())
        layers.append(
            LayerProfile(
                name=name,
                fw_ms=timer.fw_ms.get(name, 0.0),
                bw_ms=timer.bw_ms.get(name, 0.0),
                output_shape=shape,
            )
        )

    def build_extra(modules: List[Tuple[str, torch.nn.Module]]) -> Optional[ExtraProfile]:
        if not modules:
            return None
        name = modules[0][0]
        return ExtraProfile(
            name=name,
            fw_ms=timer.fw_ms.get(name, 0.0),
            bw_ms=timer.bw_ms.get(name, 0.0),
        )

    return ProfileResult(
        layers=layers,
        prologue=build_extra(prologue_modules),
        epilogue=build_extra(epilogue_modules),
    )
