#!/usr/bin/env bash
# Xcode build phase of the ShanjieKeyboard target: copies the engine's data files into the extension bundle.
# Mirrors scripts/build-app.sh for the files the engine needs; no trigram.sjt3, no tinylm.sjtl and no digits.tsv (iOS memory, ios-keyboard.md S-iOS-0; digits.md A.3.2: a missing digits.tsv only turns the digit feature off).
set -euo pipefail
REPO="$SRCROOT/.."
DEST="$TARGET_BUILD_DIR/$UNLOCALIZED_RESOURCES_FOLDER_PATH"
for f in bigram.sjlm classes.sjc kn.sjkn; do
  [ -f "$REPO/data/lm/$f" ] || { echo "error: data/lm/$f is missing; see docs/data-files.md" >&2; exit 1; }
done
mkdir -p "$DEST"
for f in mcbpmf-data.txt overlay-add.tsv sandhi-add.tsv names-add.tsv demote.tsv; do
  cp "$REPO/data/lexicon/$f" "$DEST/"
done
cp -L "$REPO/data/lm/bigram.sjlm" "$REPO/data/lm/classes.sjc" "$REPO/data/lm/kn.sjkn" "$DEST/"
# Incremental builds keep a digits.tsv copied before it was excluded; remove the stale one.
rm -f "$DEST/digits.tsv"
