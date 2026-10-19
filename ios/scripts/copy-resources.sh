#!/usr/bin/env bash
# Xcode build phase of the ShanjieKeyboard target: copies the engine's data files into the extension bundle.
# Mirrors scripts/build-app.sh for the files the engine needs; no trigram.sjt3 and no tinylm.sjtl (iOS memory, ios-keyboard.md S-iOS-0).
set -euo pipefail
REPO="$SRCROOT/.."
DEST="$TARGET_BUILD_DIR/$UNLOCALIZED_RESOURCES_FOLDER_PATH"
for f in bigram.sjlm classes.sjc kn.sjkn; do
  [ -f "$REPO/data/lm/$f" ] || { echo "error: data/lm/$f is missing; see docs/data-files.md" >&2; exit 1; }
done
mkdir -p "$DEST"
for f in mcbpmf-data.txt overlay-add.tsv sandhi-add.tsv names-add.tsv demote.tsv digits.tsv; do
  cp "$REPO/data/lexicon/$f" "$DEST/"
done
cp -L "$REPO/data/lm/bigram.sjlm" "$REPO/data/lm/classes.sjc" "$REPO/data/lm/kn.sjkn" "$DEST/"
