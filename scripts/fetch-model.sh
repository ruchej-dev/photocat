#!/usr/bin/env bash
# Ручной запасной путь: скачать веса CLIP в стандартный кэш photocat.
# Обычно модель скачивается из «Настроек» приложения (с согласием, ADR 0015).
# Запускать вручную: bash scripts/fetch-model.sh
set -euo pipefail

CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/photocat/models/clip-vit-b32"
BASE_V="https://huggingface.co/Xenova/clip-vit-base-patch32/resolve/main"
BASE_T="https://huggingface.co/sentence-transformers/clip-ViT-B-32-multilingual-v1/resolve/main"

# файл|url|sha256
FILES=(
  "vision.onnx|$BASE_V/onnx/vision_model_quantized.onnx|583fd1110a514667812fee7d684952aaf82a99b959760c8d7dca7e0ab9839299"
  "text.onnx|$BASE_T/onnx/model_quint8_avx2.onnx|fbc8fbeaa5237d96bd1bf430057c70d34de8334a463e933a5faa305f6caeed9c"
  "tokenizer.json|$BASE_T/tokenizer.json|5b4e1a8171c81dfd666ae40265b9530c6e0b3d53923fe8ac493dcc84229adf81"
  "text_dense.safetensors|$BASE_T/2_Dense/model.safetensors|d12568dc7300970a4d3dbb49068ad16cd89b99840b74b026f8e48071e9414f74"
)

mkdir -p -- "$CACHE"
for item in "${FILES[@]}"; do
  IFS='|' read -r name url want <<< "$item"
  path="$CACHE/$name"
  if [[ -f $path ]] && [[ $(sha256sum "$path" | cut -d' ' -f1) == "$want" ]]; then
    echo "уже есть: $name"
    continue
  fi
  echo "скачиваю $name…"
  curl -fL --retry 3 --max-time 900 -o "$path.tmp" "$url"
  got=$(sha256sum "$path.tmp" | cut -d' ' -f1)
  if [[ $got != "$want" ]]; then
    rm -f -- "$path.tmp"
    echo "ОШИБКА: sha256 $name не совпал: $got, ожидался $want" >&2
    exit 1
  fi
  mv -f -- "$path.tmp" "$path"
done

echo
echo "модель CLIP готова: $CACHE"
du -sh -- "$CACHE"
echo "проверка в приложении: ./target/release/photocat embed"
