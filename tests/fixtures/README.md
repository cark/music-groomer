# Synthetic audio fixtures

The files under `audio/` contain 0.25 seconds of generated silence. They are
small test seeds, not copied music. Tests copy a seed into a temporary directory
before adding or changing tags.

Regenerate them only through the pinned development environment:

```text
nix develop -c tests/fixtures/generate-audio.sh
```

The script deliberately strips input metadata and enables FFmpeg's bit-exact
flags. A fixture regeneration should be reviewed as a binary test-data change;
byte-for-byte output can still vary when the pinned FFmpeg or codec libraries
change.
