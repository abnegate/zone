#!/bin/sh
# Stand-in trainer: writes a regular file where the pipeline expects the adapter,
# so screening, selection and the receipt all run for real. It trains nothing.
printf 'zone-live-verification-adapter' > "$ZONE_TRAIN_OUTPUT"
