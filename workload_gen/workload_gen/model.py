from dataclasses import dataclass
from typing import Optional

import torch
from torch import nn

from .config import ModelSpec


class EmbeddingLayer(nn.Module):
    def __init__(self, vocab_size: int, hidden_size: int, max_position: int, dropout: float):
        super().__init__()
        self.token_embed = nn.Embedding(vocab_size, hidden_size)
        self.pos_embed = nn.Embedding(max_position, hidden_size)
        self.dropout = nn.Dropout(dropout)

    def forward(self, input_ids: torch.Tensor) -> torch.Tensor:
        seq_len = input_ids.shape[1]
        positions = torch.arange(seq_len, device=input_ids.device).unsqueeze(0)
        positions = positions.expand(input_ids.shape[0], seq_len)
        x = self.token_embed(input_ids) + self.pos_embed(positions)
        return self.dropout(x)


class TransformerBlock(nn.Module):
    def __init__(
        self,
        hidden_size: int,
        num_heads: int,
        ffn_hidden_size: int,
        dropout: float,
        is_causal: bool,
    ):
        super().__init__()
        self.is_causal = is_causal
        self.ln1 = nn.LayerNorm(hidden_size)
        self.attn = nn.MultiheadAttention(hidden_size, num_heads, dropout=dropout, batch_first=True)
        self.ln2 = nn.LayerNorm(hidden_size)
        self.mlp = nn.Sequential(
            nn.Linear(hidden_size, ffn_hidden_size),
            nn.GELU(),
            nn.Linear(ffn_hidden_size, hidden_size),
        )

    def forward(self, x: torch.Tensor, attn_mask: Optional[torch.Tensor]) -> torch.Tensor:
        residual = x
        x = self.ln1(x)
        attn_out, _ = self.attn(x, x, x, attn_mask=attn_mask, need_weights=False)
        x = residual + attn_out
        residual = x
        x = self.ln2(x)
        x = self.mlp(x)
        return residual + x


class TransformerModel(nn.Module):
    def __init__(self, spec: ModelSpec, dropout: float, is_causal: bool):
        super().__init__()
        self.spec = spec
        self.is_causal = is_causal
        self.prologue = EmbeddingLayer(
            vocab_size=spec.vocab_size,
            hidden_size=spec.hidden_size,
            max_position=spec.max_position,
            dropout=dropout,
        )
        self.layers = nn.ModuleList(
            [
                TransformerBlock(
                    hidden_size=spec.hidden_size,
                    num_heads=spec.num_heads,
                    ffn_hidden_size=spec.ffn_hidden_size,
                    dropout=dropout,
                    is_causal=is_causal,
                )
                for _ in range(spec.num_layers)
            ]
        )
        self.epilogue = nn.LayerNorm(spec.hidden_size)

    def forward(self, input_ids: torch.Tensor) -> torch.Tensor:
        if input_ids.shape[1] > self.spec.max_position:
            raise ValueError(
                f"sequence length {input_ids.shape[1]} exceeds max_position {self.spec.max_position}"
            )
        x = self.prologue(input_ids)
        attn_mask = None
        if self.is_causal:
            seq_len = input_ids.shape[1]
            mask = torch.triu(torch.ones(seq_len, seq_len, device=input_ids.device), diagonal=1)
            attn_mask = mask.bool()
        for layer in self.layers:
            x = layer(x, attn_mask=attn_mask)
        return self.epilogue(x)


def build_minimal_model(spec: ModelSpec) -> TransformerModel:
    model_type = spec.model_type.lower()
    if "bert" in model_type:
        is_causal = False
    else:
        is_causal = True
    return TransformerModel(spec, dropout=0.0, is_causal=is_causal)
